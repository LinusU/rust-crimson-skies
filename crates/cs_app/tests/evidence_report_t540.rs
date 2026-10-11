//! Evidence-report harness for task #540 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_t540_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T540 \
//!      cargo test --workspace --locked -- accept_t540_ --include-ignored \
//!      2>&1 | tee private/evidence/T540/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.) `CS_EVIDENCE_DIR` must be set **in this step**,
//!    while the acceptance suite runs: the retail content test derives its
//!    `binding-subtree-census.json` census into that directory only while the
//!    variable is set — so without it step 2 finds the directory holding no
//!    census and panics on the missing artifact.
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T540 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t540_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test evidence_report_t540 -- --ignored
//!    ```
//!    (`CS_EVIDENCE_DIR` tells this harness where step 1's run left the
//!    artifacts it must read, hash and re-check, so the census this report
//!    hashes is this run's own; `CS_EVIDENCE_ARGV` stays the acceptance
//!    command's bare argv, because an environment prefix is not part of a
//!    command's argv.)
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T540/acceptance.json \
//!      --artifact-root private/evidence/T540 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T540.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the content census the
//! retail acceptance test derived over that installation, and `rustc
//! --version` and `Cargo.lock`. Nothing is typed in by hand.
//!
//! The second artifact is a **second production observation**: this harness
//! re-runs the whole measured chain itself — `decode_interp` over the real
//! `ZBD/interp.zbd`, `discover_cockpit_bindings` over `support\cockpit.gw`,
//! `discover_airframe_roster` over `support\planes.gw`, `read_gamez_nodes` over
//! `ZBD/planes.zbd`, `audit_cockpit_coverage`, then `resolve_cockpit_content` —
//! and pins the measured counts again: 220 declared `(airframe, binding)`
//! pairs, every one resolved to drawable subtree content, 44 pairs whose
//! *direct* state stays `NoMesh` or `Ambiguous`, 24 distinct drawable cockpit
//! nodes per airframe. A stale or hand-written census cannot fake those
//! numbers, because this run recomputes them.
//!
//! The `capabilities` are checked, not assumed: `retail` is carried by the one
//! test that reads `$CS_GAME_DIR` and `synthetic` by the eight unignored
//! authored-corpus tests; every one must appear in the recorded log and pass.
//!
//! The report's `unknowns` are this task's own **blockers**, and they are
//! empty because the acceptance run passed. What the pass deliberately does
//! not answer — where the pilot's eye sits (task #542), what
//! `CameraSetHorizon` does at runtime (task #541), and whether the original
//! engine ever draws these descendants — stays machine-readable in the census
//! artifact and written out in
//! `docs/findings/2026-10-11-t540-cockpit-binding-subtree-content.md`. The
//! claim is `implemented`, never `checked` or `verified_original`.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::camera::coverage::{
    CockpitAirframe, CockpitBindingClaim, CockpitContentReport, CockpitCoverageReport,
    CockpitNodeCoverage, audit_cockpit_coverage, discover_cockpit_bindings,
    resolve_cockpit_content,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::scene::{
    AirframeDeclaration, PartRole, RosterDeclarations, RosterRoleRule, discover_airframe_roster,
};
use cs_formats::gamez::{GameZNodes, read_gamez_nodes};
use cs_formats::interp::decode_interp;
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

/// The acceptance tests whose `retail` capability this report declares: it
/// reads `$CS_GAME_DIR`.
const REQUIRED_TESTS: &[&str] =
    &["accept_t540_retail_every_declared_binding_names_drawable_geometry_in_its_own_subtree"];

/// The synthetic half of the suite, which must be present beside the retail
/// one: an evidence report that only ever ran capability-bound tests would not
/// show that the pass's contract is pinned without them.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_t540_a_nomesh_container_names_its_subtrees_drawable_geometry",
    "accept_t540_a_bound_leafs_subtree_is_the_leaf_itself",
    "accept_t540_an_ambiguous_binding_names_each_candidates_own_geometry",
    "accept_t540_absent_and_meshless_leaves_report_contentless_subtrees",
    "accept_t540_a_bindings_content_never_leaves_its_own_airframe",
    "accept_t540_the_airframe_union_counts_a_shared_node_once",
    "accept_t540_a_mismatched_or_truncated_archive_is_refused_not_walked_past",
    "accept_t540_the_content_report_carries_the_audit_states_verbatim",
];

/// The derived subtree census the retail acceptance test wrote.
const CENSUS_ARTIFACT: &str = "binding-subtree-census.json";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t540_writes_the_acceptance_report() {
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
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested, and the
    // census artifact must have been derived from it.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
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
        "no `accept_t540_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in REQUIRED_TESTS.iter().chain(SYNTHETIC_TESTS) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: T540 declares `retail`, so step 1 must run with \
                     `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        REQUIRED_TESTS.len() + SYNTHETIC_TESTS.len(),
        "and every `accept_t540_` test in the selection is one this report names: {:?}",
        suite
            .assertions
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's census is built on.
    let found =
        discover(&game_dir).expect("production discovery must read the original installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The census the retail acceptance test wrote over that installation. It
    // is checked against the tree and the hashes rather than trusted, because
    // a file left over from an earlier commit would otherwise be reported as
    // this run's.
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    let census = fs::read_to_string(&census_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR so the retail content test \
             writes its derived census",
            census_path.display()
        )
    });
    for needle in [
        format!("\"candidate_tree\":\"{candidate_tree}\""),
        format!("\"install_sha256\":\"{install_sha256}\""),
        format!("\"content_sha256\":\"{content_sha256}\""),
        "\"declared_pairs\":220".to_owned(),
        "\"resolved_pairs\":220".to_owned(),
    ] {
        assert!(
            census.contains(&needle),
            "the census does not describe this run ({needle:?}); a stale artifact cannot be \
             reported as measured evidence"
        );
    }

    // The second production observation: the whole measured chain, re-run
    // inside this harness — so the numbers the report claims were recomputed
    // here rather than trusted from the test's own output.
    let manifest_sha256 = |key: &str| {
        found
            .manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == key)
            .map(|row| row.sha256)
            .unwrap_or_else(|| panic!("{key} is inventoried"))
    };
    let (coverage_again, content_again, nodes_again) = remeasure(
        &game_dir,
        &manifest_sha256("zbd/interp.zbd"),
        &manifest_sha256("zbd/planes.zbd"),
        fingerprint(&found.manifest),
    );
    assert_remeasure(&coverage_again, &content_again, &nodes_again);

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T540\",\n\
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
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T540\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The review method the report records: what was run, against what, and what
/// this evidence does and does not claim.
const REVIEW_METHOD: &str = "the acceptance suite run locally with `CS_GAME_DIR` set, every \
    `accept_t540_` test selected by prefix with `--include-ignored`; this harness derives every \
    field from the recorded log, production discovery of $CS_GAME_DIR, the subtree census the \
    retail test derived over that installation with production readers \
    (`cs_formats::interp::decode_interp`, `cs_content::scene::discover_airframe_roster`, \
    `cs_formats::gamez::read_gamez_nodes`, `cs_app::camera::coverage::resolve_cockpit_content`), \
    and a second full production re-run of that same chain inside the harness itself \
    (assert_remeasure), which re-derives 220 declared pairs, 220 subtree-resolved pairs, 44 \
    direct-unresolved pairs and 24 drawable cockpit nodes per airframe rather than trusting the \
    log; the census artifact is checked against this report's own tree, install hash and content \
    hash before it is referenced, so a stale file cannot be reported as this run's. Validated \
    with tools/validate_evidence.py --require-pass. LIMITS OF WHAT WAS MEASURED: `retail` here is \
    read access to the original's files, never a run of the original; whether the original's \
    engine draws these descendants is unrecorded; where the pilot's eye sits is still undeclared \
    (task #542); what CameraSetHorizon does at runtime is still unmeasured (task #541); the \
    `mesh_index` a drawable node stores is an association into the archive's mesh array, and the \
    acceptance run checked each named slot against the real mesh section, but whether the \
    original draws that association is, again, a runtime question. `claim` is `implemented` \
    only. `candidate_tree` is the tree of the commit the suite ran on; the only later delta is \
    this report's own copy under docs/findings/evidence/, whose bytes are that file.";

/// The claim id the harness's own measured provenance is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("t540.evidence-harness").expect("a valid claim id")
}

/// Provenance over a real byte range of a real file — the same spans the
/// acceptance tests' claims carry, so the harness's re-measurement describes
/// the same bytes.
fn retail_provenance(
    install_sha256: ContentHash,
    container: &str,
    offset: u64,
    length: u64,
) -> Provenance {
    let span = SourceSpan::new(install_sha256, container, None, offset, length, None)
        .expect("a span over real bytes is valid");
    Provenance::new(claim(), ClaimStatus::ObservedTool, Some(span))
        .expect("an observed_tool claim with a source span is valid")
}

/// Re-runs the task's whole measured chain over `$CS_GAME_DIR` — the binding
/// discovery, the roster, the coverage audit and the content pass — as this
/// harness's own observation. Returns the coverage report, the content report
/// and the node array both were measured over.
fn remeasure(
    game_dir: &Path,
    interp_sha256: &ContentHash,
    planes_sha256: &ContentHash,
    install_sha256: ContentHash,
) -> (CockpitCoverageReport, CockpitContentReport, GameZNodes) {
    let container_label = "ZBD/interp.zbd";
    let bytes = fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    assert_eq!(
        sha256(&bytes),
        *interp_sha256,
        "the bytes this re-measurement walks are the manifest's own"
    );
    let decoded = decode_interp(&mut ParseContext::with_defaults(container_label), &bytes)
        .expect("the real container decodes");
    let claim = CockpitBindingClaim::try_new(
        "support\\cockpit.gw",
        "FindNode",
        "FindSubNode",
        "%player_plane%",
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the real claim is well formed");
    let bindings = discover_cockpit_bindings(
        &decoded,
        &claim,
        container_label,
        retail_provenance(install_sha256, container_label, 12_850, 5_713),
    )
    .expect("the claimed script is the one the container holds");

    let roster = discover_airframe_roster(
        &decoded,
        &RosterDeclarations::new(vec![AirframeDeclaration {
            script: "support\\planes.gw".to_owned(),
            bind_command: "set".to_owned(),
            include_command: "source".to_owned(),
            write_command: "GameZWriteZBDFile".to_owned(),
            create_command: "NewObject3D".to_owned(),
            container_variable: "ZBDFile".to_owned(),
            root_variable: "planeOutput".to_owned(),
            model_variable: "planeInput".to_owned(),
            required_roles: vec![RosterRoleRule {
                role: PartRole::Cockpit,
                provenance: retail_provenance(install_sha256, container_label, 187_904, 392),
            }],
            provenance: retail_provenance(install_sha256, container_label, 37_838, 8_650),
        }])
        .expect("one declaration naming one script is valid"),
    )
    .expect("the real roster discovery reports findings, not a contradiction");
    let airframes: Vec<CockpitAirframe> = roster
        .discovered()
        .iter()
        .map(CockpitAirframe::from)
        .collect();

    let archive_label = "ZBD/planes.zbd";
    let planes = fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared airframe archive must be there");
    assert_eq!(
        sha256(&planes),
        *planes_sha256,
        "the node array this re-measurement walks is the manifest's own"
    );
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(archive_label), &planes)
        .expect("the airframe archive's node array reads");
    let archive_id = ContentId::from_source(
        ContentKind::InstallFile,
        &cs_content::catalog::baseline::install_file_key(archive_label),
    )
    .expect("the normalized install-file key is a valid id key");

    let coverage = audit_cockpit_coverage(&bindings, &airframes, &nodes, &archive_id)
        .expect("every declared root exists in the real archive");
    let content = resolve_cockpit_content(&coverage, &nodes, &archive_id)
        .expect("the content pass resolves over the archive the audit measured");
    (coverage, content, nodes)
}

/// The measured corpus numbers, asserted against the harness's own re-run —
/// every one of them is a number the acceptance suite pinned and this harness
/// recomputes independently.
fn assert_remeasure(
    coverage: &CockpitCoverageReport,
    content: &CockpitContentReport,
    nodes: &GameZNodes,
) {
    assert_eq!(content.rows().len(), 11, "the eleven declared airframes");
    assert_eq!(content.declared_pairs(), 220);
    assert_eq!(content.resolved_pairs(), 220);
    assert_eq!(content.unresolved_pairs().count(), 0);
    assert!(content.is_complete());
    let direct_unresolved: usize = coverage
        .rows()
        .iter()
        .map(|row| row.unresolved().len())
        .sum();
    assert_eq!(
        direct_unresolved, 44,
        "the audit's forty-four unresolved pairs are the ones this task's \
         pass resolves to content"
    );
    for row in content.rows() {
        assert_eq!(row.bindings().len(), 20);
        assert_eq!(
            row.drawable_nodes().len(),
            24,
            "{} names twenty-four distinct drawable cockpit nodes",
            row.airframe()
        );
        let gun = row.binding("gungauge").expect("declared");
        assert!(matches!(gun.coverage(), CockpitNodeCoverage::NoMesh { .. }));
        assert_eq!(gun.subtrees()[0].subtree_nodes(), 10);
        assert_eq!(gun.subtrees()[0].geometry().len(), 8);
        let missile = row.binding("missilegauge").expect("declared");
        assert!(matches!(
            missile.coverage(),
            CockpitNodeCoverage::NoMesh { .. }
        ));
        assert_eq!(missile.subtrees()[0].subtree_nodes(), 14);
        assert_eq!(missile.subtrees()[0].geometry().len(), 12);
        for name in ["4char_ammo", "6char_type"] {
            let binding = row.binding(name).expect("declared");
            assert!(matches!(
                binding.coverage(),
                CockpitNodeCoverage::Ambiguous { .. }
            ));
            assert_eq!(binding.subtrees().len(), 2);
            assert!(
                binding
                    .subtrees()
                    .iter()
                    .all(|subtree| subtree.subtree_nodes() == 1 && subtree.geometry().len() == 1),
                "{name}: each candidate is a leaf that draws itself"
            );
        }
        // Every drawable node index the pass names is in the node array this
        // harness read — containment of the report's own claims.
        for binding in row.bindings() {
            for subtree in binding.subtrees() {
                for node in subtree.geometry() {
                    assert!(
                        nodes.get(node.node_index()).is_some(),
                        "{}'s {} names node {} inside the array",
                        row.airframe(),
                        binding.node(),
                        node.node_index()
                    );
                }
            }
        }
    }
    // And across airframes, no drawable node index is attributed twice.
    for (index, row) in content.rows().iter().enumerate() {
        let indices: BTreeSet<u32> = row
            .drawable_nodes()
            .iter()
            .map(|node| node.node_index())
            .collect();
        for other in &content.rows()[index + 1..] {
            let others: BTreeSet<u32> = other
                .drawable_nodes()
                .iter()
                .map(|node| node.node_index())
                .collect();
            assert!(
                indices.is_disjoint(&others),
                "{} and {} share drawable node indices",
                row.airframe(),
                other.airframe()
            );
        }
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_t540.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T540` written relative to the
/// workspace root in the module doc must be re-anchored here.
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

/// Extracts the libtest summaries and the per-test results of the
/// `accept_t540_` tests from a recorded `cargo test` output.
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
        // A status on its own own line completes the earliest test that was
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
            let full = &after[..separator];
            if !full.contains("accept_t540_") {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
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

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
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

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"binding-subtree-census.json\"]}}",
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

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
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
    let day_of_era = z - era * 146_097;
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
