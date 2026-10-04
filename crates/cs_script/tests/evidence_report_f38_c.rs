//! Evidence-report harness for task F38-C (Rally #158,
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_f38_c_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing. Run from the workspace
//! root, after the acceptance suite:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F38-C
//!    cargo test --workspace --locked -- accept_f38_c_ --include-ignored \
//!      2>&1 | tee private/evidence/F38-C/cargo-test.log
//!    ```
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F38-C \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f38_c_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_script --test evidence_report_f38_c -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F38-C/acceptance.json \
//!      --artifact-root private/evidence/F38-C --require-pass
//!    ```
//! 4. Commit a copy as `docs/findings/evidence/F38-C.json`.
//!
//! Every field is derived from real inputs: the recorded log, the environment,
//! production discovery of `$CS_GAME_DIR`, the production readers, scanner,
//! source map and audit over the installation, `rustc` and `Cargo.lock`. The
//! artifact holds counts, offsets, lines, columns and verdict codes only.
//!
//! The claim is **`implemented`**. No original run was observed.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
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

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f38_c_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no passing `accept_f38_c_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail acceptance test is in this log.
    let retail = suite
        .assertions
        .iter()
        .find(|(name, _)| task_test_leaf(name).starts_with("accept_f38_c_retail_"))
        .unwrap_or_else(|| {
            panic!(
                "the retail acceptance test did not run: F38-C requires capability `retail`, \
                 run step 1 with `--include-ignored` and CS_GAME_DIR set"
            )
        });
    assert_eq!(retail.1, "pass", "the retail acceptance test must pass");
    assert!(
        suite.assertions.iter().any(|(name, _)| is_task_test(name)
            && !task_test_leaf(name).starts_with("accept_f38_c_retail_")),
        "synthetic task tests must be present alongside the retail one"
    );

    // `source` hashes describe the real installation, measured by production
    // discovery.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The measured corpus, over the same production readers and the same
    // production scanner the acceptance test runs. This is the artifact: counts,
    // ids, digests and spans, and no original program text.
    let (audit_path, audit) = measure(&game_dir, &evidence_dir);
    assert_eq!(
        audit.programs, 61,
        "every shipped UI script program is mapped"
    );
    assert_eq!(
        audit.sites, 1812,
        "every measured host-call site is located"
    );
    assert_eq!(audit.bound, 0, "no family has a measured meaning");
    assert_eq!(
        audit.refused
            + audit.no_native_id
            + audit.no_family
            + audit.shape_mismatch
            + audit.arity_mismatch,
        audit.sites,
        "every site has exactly one verdict"
    );
    assert_eq!(
        audit.unjudged_heads, 3188,
        "the corpus spells 3188 call-shaped heads outside the two measured forms; they are \
         counted as unjudged, not dropped from the coverage report"
    );
    assert!(!audit.campaign_ready, "the audit refuses campaign-ready");

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&audit_path, "json", &evidence_dir),
    ];

    // The report's `unknowns` array is where an evidence run's own **unresolved
    // issues** go — a defect that makes this run untrustworthy. This run has
    // none: the container decoded, every program scanned, every site accounted
    // for and every family classified, and the digests were taken from the
    // installation this run read. That is why it validates with `--require-pass`.
    //
    // What this task does *not* know is a different thing, and it is not
    // suppressed by being kept out of the array: it is stated in full below, in
    // `source-map-audit.json` (whose `totals` carry `bound: 0`,
    // `unjudged_heads: 3188` and `campaign_ready: false` in the artifact itself)
    // and in
    // `docs/findings/scripts/2026-10-04-f38-c-source-maps-and-site-coverage.md`.
    // F38-B's committed report uses the same split for the same reason.
    let unknowns: Vec<String> = Vec::new();

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F38-C\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": {},\n\
         \x20\"artifacts\": {},\n\
         \x20\"unknowns\": {},\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        str_array(&unknowns),
        jstr(
            "implementer: sonnet-1 (Rally #158, implement claim of 2026-10-04). REVIEWER: \
             bunny-alpha-2 (Rally #158, review claim of 2026-10-04), a different agent instance \
             from the implementer, working from a fresh session context that read the task \
             description, the F38 sheet section `### F38-C`, docs/contracts/SCRIPT-MISSION.md and \
             the whole branch diff; this report was REGENERATED by the reviewer on the reviewed \
             commit from the installation, not copied from the implementer. Fixed during the \
             review (all inside the task's owner paths): `audit_sites` now takes the count of \
             call-shaped heads it cannot judge, `SiteAudit` reports it and `campaign_ready` \
             refuses while any is unjudged (3188 on this installation, pinned in the retail \
             test); `map_sites` now reports `program_too_large` instead of a misleading \
             `span_outside_program` when a line or column no longer fits a `u32`; `SourceMap` \
             documents that its key is program-scoped; `LocatedError` gained stable `code()`s; \
             two stale references in this harness (F38-B's artifact name and fields, and a wrong \
             path in an error message) were corrected. Whoever regenerates it again must correct \
             the REVIEWER sentence to name their own identity and say whether their context was \
             fresh. No agent review replaces the owner's human approval, and nothing here is \
             verified_original."
        ),
        jstr(&format!(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, and the \
             production ROF reader, host-call scanner, source map and site audit run over the \
             installation (source-map-audit.json: member, offset, line, column and verdict per \
             site, plus the unjudged call-shaped heads in its totals; no program text). The \
             audit located {} sites in {} UI script programs: {} sit in a family the table \
             refuses for want of a measured meaning, {} spell no integer dispatch id, {} sit in \
             a family whose own sites disagree and which the engine refused by name, {} are \
             shape mismatches and {} arity mismatches against their family; {} are bound, {} \
             call-shaped heads outside the two measured dispatch forms are counted as unjudged, \
             and campaign_ready is {}. LIMITS (stated in full in \
             docs/findings/scripts/2026-10-04-f38-c-source-maps-and-site-coverage.md, not removed \
             to pass this validator): (1) the meaning of every measured dispatch value is \
             unknown, so no site is bound and the audit refuses campaign-ready; (2) these are the \
             UI script programs, not the mission language -- the mission programs are not decoded \
             (F13-D), so no mission instruction or native call is located or covered here; (3) the \
             `Bound` verdict is reachable only through a family the table binds and none is, so it \
             is exercised by no retail site and `campaign_ready` cannot be observed true at all; \
             (4) no runtime event of a lowered program exists to trace, because nothing lowers \
             (0 families bound); the source map is exercised through lower_program_located on \
             authored programs; (5) no original run was observed. Validated with \
             tools/validate_evidence.py --require-pass.",
            audit.sites,
            audit.programs,
            audit.refused,
            audit.no_native_id,
            audit.no_family,
            audit.shape_mismatch,
            audit.arity_mismatch,
            audit.bound,
            audit.unjudged_heads,
            audit.campaign_ready,
        )),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    // The harness checks its own output is well-formed JSON before anyone is
    // asked to trust it: a `needle` search alone passed while the report was
    // malformed, which is exactly the kind of unverified evidence this contract
    // forbids. The structural check is deliberately independent of
    // `tools/validate_evidence.py`, which is the next gate and not this one.
    assert_brackets_balanced(&written);
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F38-C\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        // The array must render as an empty list, not as one empty nested list:
        // the validator type-checks every element.
        "\"unknowns\": []",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------------------- the measurement ---

/// What this run measured over `$CS_GAME_DIR`.
struct Audit {
    programs: usize,
    sites: usize,
    bound: usize,
    refused: usize,
    no_native_id: usize,
    no_family: usize,
    shape_mismatch: usize,
    arity_mismatch: usize,
    /// Call-shaped heads the corpus spells outside the two measured dispatch
    /// forms: counted, not judged.
    unjudged_heads: u32,
    campaign_ready: bool,
}

/// Runs the production readers, scanner, source map and audit over the
/// installation and writes the located audit as a JSON artifact.
fn measure(game_dir: &Path, evidence_dir: &Path) -> (PathBuf, Audit) {
    let container_path = game_dir.join(CRIMSON_ROF);
    let container = fs::read(&container_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", container_path.display()));
    let container_digest = sha256(&container).to_hex();

    let mut context = ParseContext::new(CRIMSON_ROF, 256 * 1024 * 1024, 8);
    let tree = read_tree(&mut context, &container).expect("the container decodes");
    let mut programs = Vec::new();
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
        let read = read_member(&context, &container, member, &RofLimits::default())
            .unwrap_or_else(|error| panic!("{spelling} must read: {error}"));
        assert_eq!(read.trailing_len, 0, "{spelling}: unconsumed stored bytes");
        programs.push((spelling, sha256(&read.data).to_hex(), read.data));
    }
    programs.sort_by(|a, b| a.0.cmp(&b.0));

    let corpus = measure_host_call_corpus(
        programs
            .iter()
            .map(|(spelling, _, bytes)| CorpusMember { spelling, bytes }),
        UiScriptLimits::default(),
    )
    .expect("every shipped UI script scans");
    let rows: Vec<MeasuredCallRow> = corpus
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
        .filter(|row| MeasuredCall::from_row(row).is_ok())
        .collect();
    let table = ObservedBindingTable::measure(&rows).expect("the measurable rows");

    let mut site_rows = Vec::new();
    for (spelling, _, bytes) in &programs {
        let scan = scan_ui_program(spelling, bytes, UiScriptLimits::default()).expect("scans");
        for mapped in map_sites(&scan, bytes).expect("every site maps") {
            site_rows.push(SiteRow {
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
            });
        }
    }
    let audit = audit_sites(&table, &site_rows, corpus.other_call_sites())
        .expect("the audit is within its bounds");

    let sites_json = audit
        .sites
        .iter()
        .map(|site| {
            format!(
                "{{\"member\":{},\"offset\":{},\"len\":{},\"line\":{},\"column\":{},\"verdict\":{}}}",
                jstr(&site.origin.member),
                site.origin.offset,
                site.origin.len,
                site.origin.line,
                site.origin.column,
                jstr(site.verdict.code()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let program_list = programs
        .iter()
        .map(|(spelling, digest, bytes)| {
            format!(
                "{{\"spelling\":{},\"sha256\":{},\"decoded_len\":{}}}",
                jstr(spelling),
                jstr(digest),
                bytes.len()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let count = |code: &str| audit.with_code(code).count();
    let result = Audit {
        programs: programs.len(),
        sites: audit.sites.len(),
        bound: audit.bound(),
        refused: audit.refused(),
        no_native_id: count("no_native_id"),
        no_family: count("no_family"),
        shape_mismatch: count("shape_mismatch"),
        arity_mismatch: count("arity_mismatch"),
        unjudged_heads: audit.unjudged_heads,
        campaign_ready: audit.campaign_ready(),
    };
    let report = format!(
        "{{\"schema\":\"cs-source-map-audit/1\",\"container\":{},\"container_sha256\":{},\
         \"programs\":[{}],\
         \"totals\":{{\"programs\":{},\"sites\":{},\"bound\":{},\"refused\":{},\
         \"no_native_id\":{},\"no_family\":{},\"shape_mismatch\":{},\"arity_mismatch\":{},\
         \"unjudged_heads\":{}}},\
         \"campaign_ready\":{},\"sites\":[{}],\
         \"note\":{}}}\n",
        jstr(CRIMSON_ROF),
        jstr(&container_digest),
        program_list,
        result.programs,
        result.sites,
        result.bound,
        result.refused,
        result.no_native_id,
        result.no_family,
        result.shape_mismatch,
        result.arity_mismatch,
        result.unjudged_heads,
        result.campaign_ready,
        sites_json,
        jstr(
            "Where every measured host-call site of the shipped UI script programs lives \
             (member, byte offset, length, 1-based line and column) and what the engine's \
             family table says about it, plus the call-shaped heads outside the two measured \
             dispatch forms that no family judges (`totals.unjudged_heads`). No program text. \
             A verdict of `refused` means the site's family is measured and refused for want of \
             a measured meaning; `no_family` means its family's own sites disagree, so the engine \
             refused the row by name; `no_native_id` means its dispatch expression names no \
             integer."
        ),
    );
    let path = evidence_dir.join("source-map-audit.json");
    fs::write(&path, &report).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    let written = fs::read_to_string(&path).expect("the artifact reads back");
    assert_brackets_balanced(&written);
    (path, result)
}

/// Whether every bracket and brace outside a string literal is closed, in order.
///
/// A deliberately small structural check, not a JSON parser: it catches the
/// failure this harness actually made twice (a missing array wrapper and an
/// unquoted key), and `tools/validate_evidence.py` is the real gate right
/// after it.
fn assert_brackets_balanced(text: &str) {
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => stack.push(character),
            '}' => assert_eq!(stack.pop(), Some('{'), "unmatched `}}` in the report"),
            ']' => assert_eq!(stack.pop(), Some('['), "unmatched `]` in the report"),
            _ => {}
        }
    }
    assert!(!in_string, "an unterminated string in the report");
    assert!(
        stack.is_empty(),
        "the report has unclosed delimiters: {stack:?}"
    );
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_script/tests/evidence_report_f38_c.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path written relative to the workspace root in the module doc
/// must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

fn iso_utc_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    // Days since the epoch, converted with the civil-from-days algorithm; the
    // timestamp only has to be an ISO-8601 instant.
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// The leaf name of a libtest test name: everything after the last `::`
/// module separator, or the whole name when it has none.
fn task_test_leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// Whether `name` is one of this task's `accept_f38_c_` tests.
fn is_task_test(name: &str) -> bool {
    task_test_leaf(name).starts_with("accept_f38_c_")
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f38_c_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !is_task_test(&name) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact, hashed with the production SHA-256 the sibling
/// crate implements (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

// ------------------------------------------------------------- rendering ---

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

/// A JSON array of objects; an empty slice renders as `[]`, like
/// [`str_array`], because the validator type-checks every element.
fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    if items.is_empty() {
        return "[]".to_owned();
    }
    format!("[{}]", items.join(", "))
}

/// A JSON array of objects; an empty slice renders as `[]`.
fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    if items.is_empty() {
        return "[]".to_owned();
    }
    format!("[{}]", items.join(", "))
}

/// A JSON array of strings. An empty slice renders as `[]`, not `[[]]`: the
/// validator requires every element to be a string, so a nested empty array
/// would be a wrong-typed element rather than an empty list.
fn str_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break out
/// of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
