//! Evidence-report harness for the #593 evidence record
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! Rally #593 (F14-D-NODE-MESH-CODE) made the F14 geometry walk take its
//! node-to-mesh verdicts from the format-level check
//! (`cs_formats::gamez::NodeMeshBindings`, task #495 / F10-C.05) instead of
//! resolving `mesh_index` a second time in
//! `crates/cs_content/src/catalog/baseline.rs`, so a stored index **past** a
//! container's mesh array stays distinguishable from a named slot the array
//! holds an all-zero stub in. It used the `retail` capability: its acceptance
//! half pins the measured installation, where every stored index names a
//! present mesh and both new findings are zero, and it adds a synthetic
//! container that stores one index of each kind.
//!
//! This test is deliberately **not** named `accept_f14_d_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/T593
//!    cargo test --workspace --locked -- accept_f14_d_ --include-ignored \
//!      2>&1 | tee private/evidence/T593/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T593 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f14_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test evidence_report_t593 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T593/acceptance.json \
//!      --artifact-root private/evidence/T593 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T593.json`.
//!
//! Every field of the report is derived here from real inputs: the recorded
//! test log, production discovery of `$CS_GAME_DIR`, the production baseline
//! builder and its own report over that installation (the consumer trace), the
//! production format-level check over each geometry container's two sections
//! read by the production readers, `rustc --version` and `Cargo.lock`.
//!
//! What is asserted here is the **measured** half of #593: over the owner's
//! installation every stored `mesh_index` of all nine GameZ containers names a
//! present mesh, so the `named_slot_without_mesh` gap stays at zero and the new
//! `named_slot_past_mesh_array` gap is zero too, and neither claim id
//! (`node_mesh_slot_absent`, `node_mesh_slot_out_of_range`) appears on any row.
//! The walk's own per-container counts are cross-checked against the
//! format-level check read straight from the two production readers, which is
//! the statement that there is one implementation of the rule: if the walk's
//! `absent_meshes`/`out_of_range_meshes` ever stopped quoting
//! `NodeMeshBindings`, the two measurements would disagree and this harness
//! would fail rather than the report quietly passing.
//!
//! `unknowns` is `[]` and the report validates with `--require-pass`: the
//! **task's** acceptance is complete. The product limits stay in
//! `review.method`, in the hashed baseline-report artifact and in the findings
//! note, so a validator cannot pass by deleting them.
//!
//! The only texts this file holds are the [`review_identity`] literal and the
//! limitations quoted into `review.method`. The first is the text the committed
//! `docs/findings/evidence/T593.json` carries;
//! `tools/tests/test_evidence_review_identity.py` reads it out of exactly this
//! shape (a `-> String` function returning a literal) and cross-checks it
//! against the committed report, so the two cannot drift apart silently. A
//! reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
//! replaces the literal with it in the same commit.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::catalog::baseline::{Baseline, baseline_report_json, retail_baseline};
use cs_formats::ParseContext;
use cs_formats::gamez::{NodeMeshBindings, read_gamez_meshes, read_gamez_nodes};
use cs_types::content::{CatalogElement, ContentKind, UnsupportedReason};

/// The measured corpus totals of the nine GameZ containers, from
/// `accept_f14_d_4_retail_every_gamez_container_yields_its_nodes_and_named_meshes`:
/// one `scene_node` row per stored node record, one `mesh` row per present mesh
/// slot a node names.
///
/// They are #593's own accepted criteria rather than this file's own: the task
/// requires the measured retail totals to be unchanged, so they are asserted
/// here as well as in the acceptance test.
const MEASURED_NODE_ROWS: usize = 56_620;
const MEASURED_MESH_ROWS: usize = 17_139;

/// The tests #593 adds or changes, by the part of their name after the
/// `accept_f14_d_4_` prefix. The record is about those: a report that only
/// showed the suite passing would not say that #593's own claims ran.
const TASK_TESTS: &[&str] = &[
    "an_index_past_the_mesh_array_is_out_of_range_not_absent",
    "retail_every_gamez_container_yields_its_nodes_and_named_meshes",
];

/// The claim ids whose count over the installation this record pins at zero.
const ZERO_CLAIMS: &[&str] = &[
    "f14.d.4.baseline.node_mesh_slot_absent",
    "f14.d.4.baseline.node_mesh_slot_out_of_range",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t593_writes_the_acceptance_report() {
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

    // Every test #593 adds or changes has to be in this log, and every one of
    // them has to have passed: the record covers those claims, not the suite.
    for test in TASK_TESTS {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(test))
            .unwrap_or_else(|| {
                panic!(
                    "{test} did not run: this record covers #593's own tests, so run step 1 \
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

    // The measured corpus totals are unchanged: #593 quotes the format-level
    // check's verdicts, so no row and no count may move.
    let node_rows = rows_of(&baseline, ContentKind::SceneNode).len();
    let mesh_rows = rows_of(&baseline, ContentKind::Mesh).len();
    assert_eq!(
        node_rows, MEASURED_NODE_ROWS,
        "the measured node-record corpus"
    );
    assert_eq!(
        mesh_rows, MEASURED_MESH_ROWS,
        "the measured present mesh-record corpus"
    );

    // Both gaps are zero on this installation, and neither contains the other's
    // rows: every stored index names a present mesh.
    let node_status = status_of(&baseline, ContentKind::SceneNode);
    let mesh_status = status_of(&baseline, ContentKind::Mesh);
    assert_eq!(node_status.diagnostic, None, "every container read");
    assert_eq!(mesh_status.diagnostic, None, "every container read");
    assert_eq!(mesh_status.gaps.get("unreadable_container"), Some(&0));
    assert_eq!(
        mesh_status.gaps.get("named_slot_without_mesh"),
        Some(&0),
        "every stored index names a present mesh: no node names an absent stub"
    );
    assert_eq!(
        mesh_status.gaps.get("named_slot_past_mesh_array"),
        Some(&0),
        "every stored index is inside its container's mesh array"
    );

    // Neither claim id appears on any row, and the walk's own per-container
    // counts agree with the format-level check read straight from the two
    // production readers.
    let mut measured_absent = 0usize;
    let mut measured_out_of_range = 0usize;
    let mut containers = BTreeSet::new();
    for report in &baseline.geometry_containers {
        containers.insert(report.spelling.clone());
        assert_eq!(report.absent_meshes, 0, "{}", report.spelling);
        assert_eq!(report.out_of_range_meshes, 0, "{}", report.spelling);
        let bytes = fs::read(game_dir.join(&report.spelling))
            .unwrap_or_else(|error| panic!("{} reads: {error}", report.spelling));
        let label = report.spelling.clone();
        let mut context = ParseContext::with_defaults(label.clone());
        let nodes = read_gamez_nodes(&mut context, &bytes)
            .unwrap_or_else(|error| panic!("{label}: the node array must read: {error}"));
        let meshes = read_gamez_meshes(&mut context, &label, &bytes)
            .unwrap_or_else(|error| panic!("{label}: the mesh section must read: {error}"));
        // The one implementation of the rule, over the real bytes.
        let bindings = NodeMeshBindings::of(&nodes, &meshes);
        assert_eq!(
            bindings.unresolved(),
            0,
            "{label}: {bindings} — the format-level check reports no unresolved node"
        );
        assert!(bindings.is_complete(), "{label}: {:?}", bindings.findings);
        assert_eq!(
            bindings.bound as usize,
            nodes.mesh_index_bounds().bound,
            "{label}: the check and the node reader agree on the bound"
        );
        assert_eq!(
            bindings.slots as usize,
            meshes.slot_count(),
            "{label}: the check and the mesh reader agree on the array size"
        );
        measured_absent += report.absent_meshes;
        measured_out_of_range += report.out_of_range_meshes;
    }
    assert_eq!(
        mesh_status.rows, mesh_rows,
        "the collection status and the rows agree"
    );
    assert_eq!(
        measured_absent + measured_out_of_range,
        0,
        "the walk measured no unresolved named slot on this installation"
    );

    for element in rows_of(&baseline, ContentKind::SceneNode) {
        let claims = unknown_claims(element);
        for claim in ZERO_CLAIMS {
            assert!(
                !claims.contains(claim),
                "no row of this installation carries {claim}: {}",
                element.id
            );
        }
    }

    // Every geometry row's edge resolves inside the catalog, so the walk did not
    // drop a reference when it stopped resolving `mesh_index` itself.
    for element in rows_of(&baseline, ContentKind::SceneNode)
        .into_iter()
        .chain(rows_of(&baseline, ContentKind::Mesh))
    {
        for edge in &element.dependencies {
            assert!(
                baseline.catalog.get(&edge.target).is_some(),
                "{} points at {}, which the catalog does not hold",
                element.id,
                edge.target
            );
        }
    }
    assert_eq!(baseline.coverage.unresolved_references, 0);

    let report = baseline_report_json(&baseline);
    fs::write(&report_path, &report)
        .unwrap_or_else(|error| panic!("write {}: {error}", report_path.display()));
    for needle in [
        "\"schema\":\"cs-content-baseline/1\"",
        "\"retail\":true",
        &format!("\"scene_node\":{node_rows}"),
        &format!("\"mesh\":{mesh_rows}"),
        "\"named_slot_past_mesh_array\":0",
        "\"named_slot_without_mesh\":0",
        "\"geometry_containers\":[",
    ] {
        assert!(
            report.contains(needle),
            "the consumer report is missing {needle:?}"
        );
    }
    assert!(
        containers.len() >= 9,
        "the baseline walked the nine measured GameZ containers: {containers:?}"
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

    let measured = Measured {
        node_rows,
        mesh_rows,
        containers: containers.len(),
        absent: measured_absent,
        out_of_range: measured_out_of_range,
    };
    let written = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T593\",\n\
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
         \x20\"unknowns\": [],\n\
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
        jstr(&review_identity()),
        jstr(&review_method(&measured)),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &written).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    // A cheap self-check without a JSON dependency: the validator runs next,
    // but a structurally empty write must fail here first.
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T593\"",
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
    node_rows: usize,
    mesh_rows: usize,
    containers: usize,
    absent: usize,
    out_of_range: usize,
}

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/T593.json` carries, and it is the first string in
/// this function on purpose: `tools/tests/test_evidence_review_identity.py`
/// reads a harness's recorded identity out of exactly this shape (a `-> String`
/// function returning a literal) and cross-checks it against the committed
/// report, so the text here and the report cannot drift apart silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// then names the review that actually happened.
///
/// It states plainly what this record is and is not: one agent instance wrote
/// the change, no agent review replaces the owner's human approval, and nothing
/// here is `verified_original` — `retail` is read access to the installation's
/// own files, the original game was never run, and the format-level check this
/// task quotes measures what the files state rather than what the engine did.
fn review_identity() -> String {
    let recorded = String::from(
        "the subject of this record, Rally #593 (F14-D-NODE-MESH-CODE): implementer \
         opencode/step-5-preview-free (bunny-alpha-2, implement claim of \
         2026-10-13T05:20:56Z, resumed twice after lease expiries), with the rebases and the \
         evidence re-recording finished by an earlier Devin SWE-2/swe2-max-1 session after the \
         last expiry. Reviewer Devin SWE-2/swe2-max-1 (review claim of 2026-10-13T18:27:53Z), a \
         different agent instance with a fresh context — it did not write the change, it reviewed \
         the diff against the task's criteria, made one consistency edit inside the task's owner \
         path (the material edge's named-slot set now reads the same format-level verdicts rather \
         than a third `meshes.get` lookup), re-ran the full `accept_f14_d_` selection locally with \
         `retail` and re-recorded this report on the reviewed tree. The reviewer shares a model \
         with the earlier swe2-max-1 session that prepared the submission, so the review is \
         fresh-context but not a different model, and no agent review replaces the owner's human \
         approval. Nothing in this record is independent original-reference evidence: `retail` \
         is read access to the installation's own files, the original game was never run, and the \
         format-level check measures what the files state rather than what the engine did.",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, what it measured,
/// and the limits this report does not claim away.
///
/// The numbers are interpolated from this run's own measurements, and the
/// limitations are quoted rather than deleted from the record (the 2026-09-28
/// owner directive), so a reader of the committed report sees them next to the
/// passing assertions.
fn review_method(measured: &Measured) -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, the production \
         baseline report over that installation, the production format-level check \
         (cs_formats::gamez::NodeMeshBindings) over each geometry container's node array and mesh \
         section read by the production readers, rustc and Cargo.lock; validated with \
         tools/validate_evidence.py --require-pass. The consumer trace is \
         cs_content::catalog::baseline::retail_baseline + baseline_report_json, the same \
         functions `cs-inspect catalog --cs-path` writes, and the two measurements are \
         cross-checked against each other: the walk's per-container absent_meshes and \
         out_of_range_meshes and the mesh collection's two gaps are the format-level check's own \
         verdicts, read over the real bytes, so a second implementation that disagreed would fail \
         here. `candidate_tree` is the tree of the commit the acceptance suite and this harness \
         ran on; the only later delta is this report's own copy under \
         docs/findings/evidence/T593.json. The synthetic half of the acceptance is \
         accept_f14_d_4_an_index_past_the_mesh_array_is_out_of_range_not_absent, which authors a \
         container whose node array stores one mesh_index past its three-slot mesh array and one \
         naming an inside-the-array all-zero stub, and asserts the two are reported as two facts: \
         two claim ids on two node rows, two collection gaps, and two per-container counts. The \
         implementer verified that it fails when the two codes collapse, by making every finding \
         report `mesh_slot_absent` (killed at the per-container count) and by making the row reason \
         emit the absent claim for an out-of-range index (killed at the row's claim id).",
    );
    format!(
        "{recorded} MEASURED ON THIS TREE: {} scene_node rows and {} mesh rows over {} geometry \
         containers, and a format-level verdict of no unresolved node anywhere, so \
         named_slot_without_mesh = {} and named_slot_past_mesh_array = {} and neither claim id \
         occurs on any row. PRODUCT INCOMPLETENESS, NAMED SO A VALIDATOR CANNOT PASS BY DELETING \
         IT: the two codes are distinguishable in the machine-readable catalog, but nothing in \
         this installation exercises the second one, so the out-of-range half of the rule is \
         pinned by a synthetic container only. Affected content: the machine-readable Mesh \
         collection status and the SceneNode row reasons on any container variant that stores an \
         index past its mesh array — none in the measured installation. Resolving task: none \
         required; the code path is exercised by the synthetic acceptance test and the retail test \
         pins that the count stays zero. The F10-C.05 limitation this task closes is updated in \
         docs/findings/2026-10-03-f10-c-05-node-mesh-index-cross-check.md, and this task's own \
         note docs/findings/2026-10-13-t593-f14-d-node-mesh-code.md records what remains: that \
         `NodeMeshBindings` carries no multiplicity, that an absent slot no node names is unused \
         capacity whose engine treatment is unmeasured, that the header is not cross-checked \
         between the two sections, that which mesh a node ends up drawing stays F11's and F17-B's, \
         that `mesh_index` on a non-object record is carried and checked but claims nothing, and \
         that a container with no node array cannot be checked at all. Affected content of those: \
         a stage that needs an instance count; the engine treatment of an absent slot; a caller \
         that could pair a node array with a sibling container's mesh section; every rendering \
         and LOD decision; the mesh_index word on world, window, camera, display and light \
         records; and every synthetic F10-C.03-style fixture with no node array. Resolving tasks: \
         F11-C and F29's name-path rules, F11-D, F17-B and whoever reads a cue class out of \
         original bytes or runs the original. Nothing here is verified_original: the original \
         game was never run and no human played it. Affected content: every fidelity claim above \
         `implemented`. Resolving task: the owner, with an original run and a human review.",
        measured.node_rows,
        measured.mesh_rows,
        measured.containers,
        measured.absent,
        measured.out_of_range,
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_t593.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T593` written relative to the
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

// ------------------------------------------------------------- the catalog ---

/// The rows of one kind, by id key, in canonical order.
fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

/// The [`cs_content::catalog::baseline::CollectionStatus`] of one kind.
fn status_of(
    baseline: &Baseline,
    kind: ContentKind,
) -> &cs_content::catalog::baseline::CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("{} reports its collection status", kind.label()))
}

/// The claim ids of every `UnsupportedReason::Unknown` a row carries.
fn unknown_claims(element: &CatalogElement) -> Vec<&str> {
    element
        .unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect()
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
            // test name, so a unit test inside a module counts exactly as
            // the selection counts it.
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
        .expect("an artifact has a file name")
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
