//! Evidence-report harness for the #584 evidence record
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! Rally #584 made the shared F14-D baseline completeness total **derive** its
//! non-launchable collections from the kinds present in the catalog
//! (`cs_types::content::{CatalogRowRole, account_catalog_rows}`) instead of
//! naming each collection in a hand-maintained sum. It used the `retail`
//! capability: its acceptance half pins measurements of the owner's original
//! installation (228 inventoried files, 53 launchable roots, the
//! non-launchable collection-row total, the catalog length and the coverage
//! split), and #584 filed a findings note that argued those measurements were
//! not a new claim about original content and therefore needed no evidence
//! record. The review disagreed, so this harness writes that record instead.
//!
//! This test is deliberately **not** named `accept_f14_d_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/T584
//!    cargo test --workspace --locked -- accept_f14_d_ --include-ignored \
//!      2>&1 | tee private/evidence/T584/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T584 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f14_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_t584 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T584/acceptance.json \
//!      --artifact-root private/evidence/T584 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/T584.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, production discovery of `$CS_GAME_DIR`, the production baseline
//! builder and its own report over that installation (the consumer trace),
//! production `account_catalog_rows` over that catalog, `rustc --version` and
//! `Cargo.lock`. The only texts this file holds are the [`review_identity`]
//! literal the committed `docs/findings/evidence/T584.json` carries — which
//! names both pairs of actors behind the record, #584's implementer and
//! #584's reviewer and this harness's author and reviewer, and states plainly
//! that each pair is one agent instance, so neither review was independent —
//! and the product-completeness limitations quoted into `review.method`.
//!
//! Nothing about the totals is typed in by hand. `review.method` interpolates
//! what this run measured, and it names every source-derived collection that
//! still holds **no** row by reading `ContentKind::ALL` against the measured
//! catalog, so a collection that has not landed cannot go missing from the
//! record by being forgotten here (the 2026-09-28 owner directive). What is
//! typed in are the *floors* of [`MEASURED_COLLECTIONS`] and
//! [`MEASURED_SOURCE_DERIVED`], which are #584's own numbers from
//! `accept_f14_d_baseline.rs` rather than this file's own: they are asserted as
//! `>=` so a later collection cannot fail this harness for arriving, while a
//! collection that loses rows or disappears still does.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **task's** acceptance is complete — the total is derived from the rows, the
//! identity it has to satisfy holds over the installation, the four tests #584
//! adds or changes pass, and every selected test passed. The product
//! incompleteness is a different state and is moved, never deleted:
//! `review.method` names every source-derived collection the catalog holds no
//! row of, the hashed baseline-report artifact names the two of them the
//! baseline builder reads and reports empty (`music`, `dialogue`) with a
//! diagnostic, every populated collection has its producing stage's acceptance
//! test, and every empty one has its own follow-up task. A failing run produces
//! a failing report, which the validator rejects.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{CampaignInventory, campaign_layout};
use cs_content::catalog::baseline::{Baseline, baseline_report_json, retail_baseline};
use cs_types::content::{CatalogRowAccounting, CatalogRowRole, ContentKind, account_catalog_rows};

/// Per-collection row counts measured from the owner's installation on
/// 2026-10-03, asserted as floors.
///
/// They are #584's own `MEASURED_COLLECTIONS` in
/// `crates/cs_content/tests/accept_f14_d_baseline.rs`: eight collections it
/// measured itself, plus the 45 `stunt` and 461 `scrapbook_item` rows F14-D.8
/// measured and #584 folded into its floor when it rebased onto that stage.
///
/// They are floors rather than equalities on purpose: an exact total is the
/// hand-maintained number this task's subject removed, and a later collection
/// must be able to add rows without failing somebody else's test. A collection
/// that gains rows must not fail this record; a collection that loses rows, or
/// disappears, must.
const MEASURED_COLLECTIONS: &[(&str, usize)] = &[
    ("multiplayer_rules", 4),
    ("world", 8),
    ("faction", 11),
    ("airframe", 11),
    ("paint_mask", 184),
    ("sound", 4951),
    ("scene_node", 56620),
    ("mesh", 17139),
    ("stunt", 45),
    ("scrapbook_item", 461),
];

/// #584's own floor for the non-launchable collection rows of this
/// installation: the 78 928 it measured, plus the 506 rows F14-D.8 added and
/// #584 folded in on its rebase.
///
/// This is the same number `accept_f14_d_baseline.rs` pins, deliberately: a
/// record that floor-ed lower than the test it records would pass where the
/// test fails, which is the one thing a reviewer must not have to notice.
const MEASURED_SOURCE_DERIVED: usize = 79_434;

/// The launchable denominator of this installation, measured 2026-10-03: 24
/// campaign missions plus 29 scenario directories.
const MEASURED_LAUNCHABLE: usize = 53;

/// The inventoried regular files of this installation, measured 2026-10-03.
const MEASURED_INSTALL_FILES: usize = 228;

/// The tests #584 adds or changes, by the part of their name after the
/// `accept_f14_d_` prefix. The record is about those four: a report that only
/// showed the suite passing would not say that #584's own claims ran.
const TASK_TESTS: &[&str] = &[
    "every_content_kind_is_classified_for_the_baseline_accounting",
    "catalog_rows_are_accounted_by_kind_and_not_by_a_named_list",
    "a_collection_named_by_no_list_is_still_accounted",
    "retail_baseline_inventory_is_complete_and_never_synthetic",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t584_writes_the_acceptance_report() {
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

    // The candidate tree must be the tree that was actually tested: a stale
    // report from another commit is exactly what this check refuses.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );
    assert!(
        (candidate_tree.len() == 40 || candidate_tree.len() == 64)
            && candidate_tree
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "CS_CANDIDATE_TREE must be a hex Git tree id, got {candidate_tree:?}"
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
        (suite.assertions.len() as u64) >= suite.passed,
        "fewer per-test results than passing tests were parsed from {} — the log format was \
         not understood; inspect it rather than reporting guessed counts",
        log_path.display()
    );
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f14_d_` tests were recorded in {}",
        log_path.display()
    );

    // Every test #584 adds or changes has to be in this log, and every one of
    // them has to have passed: the record covers those claims, not the suite.
    for test in TASK_TESTS {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(test))
            .unwrap_or_else(|| {
                panic!(
                    "{test} did not run: this record covers #584's own tests, so run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(line.1, "pass", "{test}: status {}", line.1);
    }

    // `source` hashes describe the real installation, measured by the very
    // production code the task's consumer wires in.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the production baseline builder's own report over the
    // original installation, written into the evidence directory. It is the same
    // function `cs-inspect catalog --cs-path` writes.
    let report_path = evidence_dir.join("baseline-report.json");
    let baseline = retail_baseline(&game_dir)
        .expect("the production baseline must read the original installation");
    assert_eq!(
        baseline.install_sha256, install_sha256,
        "the baseline's installation fingerprint is production discovery's"
    );
    assert_eq!(baseline.content_sha256, content_sha256);
    let catalog = &baseline.catalog;

    // The measurement this task records: the completeness total, derived by the
    // production classification #584 added, from the rows the catalog holds.
    let accounting = account_catalog_rows(catalog.elements());
    assert!(
        accounting.is_complete(),
        "every catalog row falls into exactly one role: {accounting}"
    );
    assert_eq!(
        accounting.total,
        catalog.len(),
        "the accounting counts every row and no other: {accounting}"
    );
    assert_eq!(
        accounting.install_file,
        found.manifest.files.len(),
        "an install-file row is one inventoried file: {accounting}"
    );
    assert_eq!(
        accounting.install_file, MEASURED_INSTALL_FILES,
        "the original installation inventories 228 regular files, measured 2026-10-03: \
         {accounting}"
    );
    assert_eq!(
        accounting.launchable, accounting.program,
        "each launchable row is read from exactly one program archive and every program \
         archive belongs to one, so an orphan program row is a defect: {accounting}"
    );
    assert_eq!(
        accounting.launchable,
        baseline.roots.len(),
        "the declared roots are exactly the launchable rows: {accounting}"
    );
    assert_eq!(accounting.launchable, MEASURED_LAUNCHABLE);
    assert!(
        accounting.unaccounted() >= MEASURED_SOURCE_DERIVED,
        "the non-launchable collection rows measured {MEASURED_SOURCE_DERIVED} on 2026-10-03 \
         (228 inventoried files, 53 launchable rows and their 53 programs, and F14-D.8's 506 \
         stunt and scrapbook rows) and there are fewer now: {accounting}"
    );
    assert_eq!(
        catalog.len(),
        accounting.install_file + 2 * accounting.launchable + accounting.unaccounted(),
        "files plus one program row and one launchable row per mission and scenario, plus every \
         non-launchable collection row the catalog holds: {accounting}"
    );
    let collections = accounting.collections();
    for &(kind, measured) in MEASURED_COLLECTIONS {
        let held = collections
            .iter()
            .find(|(label, _)| *label == kind)
            .map(|(_, rows)| *rows)
            .unwrap_or_else(|| {
                panic!("{kind} held {measured} rows on 2026-10-03 and is gone: {accounting}")
            });
        assert!(
            held >= measured,
            "{kind} held {measured} rows on 2026-10-03 and holds {held} now: {accounting}"
        );
    }

    // Coverage: each launchable row reaches its program and the file holding
    // its bytes, everything else stays accounted instead of disappearing, and
    // nothing is playable yet.
    assert_eq!(baseline.coverage.roots, accounting.launchable);
    assert_eq!(
        baseline.coverage.reachable,
        3 * accounting.launchable,
        "each launchable reaches its program and the file holding its bytes: {accounting}"
    );
    assert_eq!(
        baseline.coverage.unreachable,
        catalog.len() - baseline.coverage.reachable,
        "{accounting}"
    );
    assert_eq!(
        baseline.coverage.unreachable_needing_classification, baseline.coverage.unreachable,
        "every unreachable row is unknown and still needs a classification: {accounting}"
    );
    assert_eq!(baseline.coverage.unresolved_references, 0);
    assert_eq!(baseline.coverage.ready, 0, "nothing is playable yet");
    assert!(!catalog.is_fully_ready());
    assert!(!catalog.is_retail_ready());
    assert_eq!(catalog.synthetic_launchable_count(), 0);
    assert_eq!(catalog.launchable_count(), accounting.launchable);
    assert_eq!(catalog.original_launchable_count(), accounting.launchable);
    assert_eq!(catalog.unsupported_count(), accounting.launchable);

    // Nothing authored reached the retail inventory, and every row is located
    // by the fingerprint production discovery measured.
    for element in catalog.elements() {
        assert!(
            element.origin.is_original(),
            "a retail catalog row cannot be {}: {}",
            element.origin.label(),
            element.id
        );
        let span = element
            .origin
            .source()
            .expect("an installation row is located by a checked span");
        assert_eq!(span.install_sha256().to_hex(), install_sha256);
    }

    // The denominator is cross-checked against two declarations that do not
    // come from the builder: the frozen F50 campaign inventory and the shared
    // campaign walk. The accounting cannot move them, and they cannot move it
    // unnoticed.
    let layout = campaign_layout(&game_dir).expect("the shared campaign walk reads the layout");
    let scenarios = baseline
        .classified_reader_dirs
        .iter()
        .filter(|dir| dir.role.is_launchable())
        .count();
    assert_eq!(
        baseline.roots.len(),
        layout.len() + scenarios,
        "the denominator declares exactly the campaign missions the shared walk finds plus the \
         scenario directories F14-D.1 classified"
    );
    let inventory_path = workspace_root().join("missions/bindings/campaign-inventory.tsv");
    let inventory = CampaignInventory::load(&inventory_path)
        .unwrap_or_else(|error| panic!("{} reads: {error}", inventory_path.display()));
    assert_eq!(
        layout.len(),
        inventory.len(),
        "the campaign part of the denominator equals the frozen F50 campaign denominator"
    );
    assert!(
        baseline.unrecognized_program_dirs.is_empty(),
        "F14-D.1 classifies every reader directory of the owner's installation; unclassified: {:?}",
        baseline.unrecognized_program_dirs
    );

    let report = baseline_report_json(&baseline);
    fs::write(&report_path, &report)
        .unwrap_or_else(|error| panic!("write {}: {error}", report_path.display()));
    for needle in [
        "\"schema\":\"cs-content-baseline/1\"",
        "\"retail\":true",
        "\"synthetic_launchable\":0",
        &format!("\"install_sha256\":\"{install_sha256}\""),
        &format!("\"launchable\":{}", accounting.launchable),
        "\"collection_status\":[",
        "\"unrecognized_program_dirs\":[",
        "\"classified_reader_dirs\":[",
    ] {
        assert!(
            report.contains(needle),
            "the consumer report is missing {needle:?}"
        );
    }
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "the retail consumer report holds no authored row"
    );

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&report_path, "json", &evidence_dir),
    ];

    let measured = Measured::of(&accounting, &baseline);
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T584\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
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
        // Deliberately empty (the brackets are the template's): see the
        // module doc — the task's own acceptance is complete, and the
        // product-completeness state is named in `review.method`, in the
        // hashed artifact and in the follow-up tasks instead of being deleted.
        "",
        jstr(&review_identity()),
        jstr(&review_method(&measured)),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next,
    // but a structurally empty write must fail here first.
    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T584\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [],",
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

/// What this run measured, so `review.method` quotes the numbers of the tested
/// tree instead of numbers typed in by hand and left to rot.
struct Measured {
    install_file: usize,
    launchable: usize,
    program: usize,
    source_derived: usize,
    total: usize,
    reachable: usize,
    unreachable: usize,
    /// The source-derived collections the catalog holds rows for.
    populated: Vec<(&'static str, usize)>,
    /// The source-derived collections that still hold **no** row.
    empty: Vec<&'static str>,
}

impl Measured {
    /// Reads the totals out of the production accounting, and the collections
    /// that hold no row out of `ContentKind::ALL`: a kind the classification
    /// calls a source-derived collection but the catalog holds no row of is
    /// named here whether or not anybody remembered it.
    fn of(accounting: &CatalogRowAccounting, baseline: &Baseline) -> Self {
        let populated = accounting.collections();
        let held: BTreeSet<ContentKind> = accounting.rows_by_kind.keys().copied().collect();
        let empty: Vec<&'static str> = ContentKind::ALL
            .iter()
            .copied()
            .filter(|kind| kind.baseline_row_role() == CatalogRowRole::SourceDerivedCollection)
            .filter(|kind| !held.contains(kind))
            .map(|kind| kind.label())
            .collect();
        assert!(
            !empty.is_empty(),
            "every source-derived collection holds a row on this installation: {} collections \
             hold rows and none is empty, so this record would be claiming a completeness the \
             catalog does not have",
            populated.len()
        );
        Self {
            install_file: accounting.install_file,
            launchable: accounting.launchable,
            program: accounting.program,
            source_derived: accounting.unaccounted(),
            total: accounting.total,
            reachable: baseline.coverage.reachable,
            unreachable: baseline.coverage.unreachable,
            populated,
            empty,
        }
    }
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/T584.json` carries, and it is the first string in
/// this function on purpose: `tools/tests/test_evidence_review_identity.py`
/// reads a harness's recorded identity out of exactly this shape
/// (`fn review_identity() -> String`, a literal) and cross-checks it against the
/// committed report, so the text here and the report cannot drift apart
/// silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// then names the review that actually happened.
///
/// The literal names two pairs, because two pairs happened: #584's
/// implementer and #584's reviewer are who the recorded measurements belong
/// to, and the harness author and the harness reviewer are who produced this
/// file. Both pairs are the same agent instance within their own task, so
/// neither review is independent original-reference evidence.
fn review_identity() -> String {
    let recorded = String::from(
        "the subject of this record, Rally #584: implementer: \
         openrouter/stealth-space-bunny-alpha (bunny-alpha-2, implement claim of \
         2026-10-03T11:03:00Z, branch commit 41833076); reviewer: \
         openrouter/stealth-space-bunny-alpha (bunny-alpha-2, review claim of \
         2026-10-03T12:15:56Z) — the same agent instance and model as the implementer, so that \
         review is not independent evidence: it is a self-review and it cannot stand in for a \
         fresh-context agent or for the owner's human approval. Its context was not fresh: the \
         review session started from the implementer's own hand-over summary and read the branch \
         diff from there. The self-review found and fixed real defects (a kind-label uniqueness \
         assertion, and the missing evidence record this task is) and re-derived the retail \
         counts from the installation rather than trusting the summary, but a review by the \
         agent that wrote the code is `checked` evidence at best and never verified_original \
         evidence. This record's own harness: implementer opencode/space-bunny-free (bunny-2, \
         Rally #587, implement claim of 2026-10-03T12:51:13Z); reviewer \
         opencode/space-bunny-free (bunny-2, Rally #587, review claim of \
         2026-10-03T16:27:45Z) — again the same agent instance and model, in a separate session \
         whose context did not carry the implementing session's, so this review is not \
         independent evidence either; it re-derived every recorded total from the installation, \
         corrected the source-derived floor that had been left at the pre-F14-D.8 measurement, \
         and regenerated this report on the rebased commit.",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, what it measured,
/// and the product-completeness limits this report does not claim away.
///
/// The totals and the empty-collection list are interpolated from this run's
/// own measurements, and the limitations are quoted rather than deleted from
/// the record (the 2026-09-28 owner directive), so a reader of the committed
/// report sees them next to the passing assertions.
fn review_method(measured: &Measured) -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, the production \
         baseline report over that installation, production cs_types::content::\
         account_catalog_rows over that catalog, rustc and Cargo.lock; validated with \
         tools/validate_evidence.py --require-pass. The consumer trace is \
         cs_content::catalog::baseline::retail_baseline + baseline_report_json, the same \
         functions `cs-inspect catalog --cs-path` writes. `candidate_tree` is the tree of the \
         commit the acceptance suite and this harness ran on; the only later delta is this \
         report's own copy under docs/findings/evidence/T584.json. The totals this record covers \
         are #584's: the completeness identity is production cs_types::content::account_catalog_rows \
         classifying every row into one of four CatalogRowRole values and its Display naming the \
         collections that do not add up, so the record states that classification's output over \
         the owner's installation rather than a sum typed into a test.",
    );
    let mut out = format!(
        "{recorded} MEASURED ON THIS TREE: {} inventoried install-file rows, {} launchable rows \
         and their {} program rows, {} source-derived collection rows, {} catalog rows in all; \
         coverage {} reachable and {} unreachable, 0 ready, 0 unresolved references. The \
         collections the catalog holds rows for are: {}.",
        measured.install_file,
        measured.launchable,
        measured.program,
        measured.source_derived,
        measured.total,
        measured.reachable,
        measured.unreachable,
        measured
            .populated
            .iter()
            .map(|(kind, rows)| format!("{kind} {rows}"))
            .collect::<Vec<String>>()
            .join(", "),
    );
    out.push_str(&format!(
        " PRODUCT INCOMPLETENESS, NAMED SO A VALIDATOR CANNOT PASS BY DELETING IT: the {} \
         source-derived collections above are {} of the {} the classification can hold; the {} \
         that still hold no row on this installation are {} — their content is inventoried as \
         bytes, not understood. The {} above each arrived as its own stage with its own acceptance \
         test and evidence record; each empty one is tracked as its own follow-up task (#559 image, \
         #561 collision_surface and #573 video among them), so this is named outstanding work \
         rather than a collection somebody forgot. #389, the umbrella task for it, was merged \
         after its first such slice (F14-D.2).",
        measured.populated.len(),
        measured.populated.len(),
        measured.populated.len() + measured.empty.len(),
        measured.empty.len(),
        measured.empty.join(", "),
        measured.populated.len(),
    ));
    out.push_str(
        &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>(),
    );
    out
}

/// The limitations this stage records instead of guessing, each naming the
/// affected content and the task that resolves it (AGENTS owner directive,
/// 2026-09-28: a limitation must survive into machine-readable evidence).
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "The derived total is a completeness property, not a fidelity claim: it says every catalog \
     row falls into exactly one role and the role totals add up to the catalog's length, not \
     that any row means what its kind says. Affected content: every row of every populated \
     collection, and in particular the identities inside them. Resolving tasks: the per-collection \
     acceptance tests that pin those identities (accept_f14_d_2_ through accept_f14_d_8_retail_…) \
     and the producing stages' own findings notes.",
    "The floors in this record are #584's own: `MEASURED_COLLECTIONS` and the 79 434 total floor \
     in accept_f14_d_baseline.rs, measured on 2026-10-03, with F14-D.8's 45 stunt and 461 \
     scrapbook rows folded in when #584 rebased onto that stage, so the source-derived total above \
     is exactly the floor that test pins. They are floors rather than equalities on purpose — an \
     exact total is the hand-maintained number #584 removed — so a collection that gains rows \
     raises them without failing either place, while a collection that loses rows or disappears \
     fails both. Affected content: whichever collection regressed. Resolving tasks: the \
     collection's own acceptance test, which pins the identities and the per-container totals.",
    "No row claims a runtime consumer and no mission program is decoded, so the coverage \
     accounting reports 0 ready and every launchable row as unsupported. Affected content: the \
     playability of all 53 roots. Resolving tasks: F37 (mission IR) and F38 (native behavior \
     bindings), which are what name a cue or an asset from a mission program.",
    "The classification itself is a design decision, not a measurement: `Material`, `Image`, \
     `Weapon`, `CustomPlane` and the other kinds no stage inserts yet are declared source-derived \
     collections, which is why the derived total cannot be forgotten but also cannot report a \
     collection that a future stage turns out to be something else. Affected content: any kind \
     whose role is wrong. Resolving tasks: the owner's spec ruling; a change to a role is one \
     line in `ContentKind::baseline_row_role` and does not compile without a catch-all arm to \
     hide behind.",
    "This record says nothing about what the original game did with the content: no original-run \
     capture, no human play, no audible or visual check. It reports what the files state. \
     Affected content: every fidelity claim above `implemented`. Resolving task: the owner, with \
     an original run and a human review.",
];

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_t584.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T584` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

/// The workspace root, located through git rather than through the package
/// layout, so the harness cannot read a neighbouring checkout by accident.
fn workspace_root() -> PathBuf {
    PathBuf::from(git(&["rev-parse", "--show-toplevel"]))
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
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
    let lock_path = workspace_root().join("Cargo.lock");
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

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f14_d_` tests from a recorded `cargo test` output.
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
        // A status on its own line completes the earliest test that was
        // started on an earlier line without an inline status.
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
        // `test <name> ... <status>`, possibly several per interleaved line.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            // `cargo test -- <prefix>` matches the prefix anywhere in the
            // test name, so a unit test inside a module counts exactly as the
            // selection counts it.
            if !name.contains("accept_f14_d_") {
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

// ------------------------------------------------------------ artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the sibling
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

// ------------------------------------------------------------ rendering ---

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
    items.join(", ")
}

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
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
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

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat` accepts
/// after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = era * 400 + year_of_era;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
