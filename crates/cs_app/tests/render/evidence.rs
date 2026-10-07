//! Evidence-report harness for tasks F17-D and F17-E-MATRIX-COVERAGE
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//! The F17-E sequence — same shape, its own prefix and artifact names — is
//! documented in the `F17-E` section below.
//!
//! This test is deliberately **not** named `accept_f17_d_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f17_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F17-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F17-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f17_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_app --test render evidence_report_f17_d -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F17-D/acceptance.json \
//!      --artifact-root private/evidence/F17-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F17-D.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the production matrix
//! this harness resolves over that installation a second time, and the PNGs the
//! GPU captures wrote. Nothing is typed in by hand.
//!
//! The `capabilities` are checked, not assumed: `retail` and `gpu` are declared
//! only because every test listed in [`REQUIRED_TESTS`] — the two that read
//! `$CS_GAME_DIR` and the one that needs an adapter — is in the recorded log and
//! passed.
//!
//! The report's `unknowns` are *this task's* blockers and are empty because the
//! acceptance run passed. The product-incompleteness state this stage measured —
//! no evidence source states a render class for original content yet, so every
//! material of the comparison set is reported `undeclared`, and no original run
//! has produced the other side of the comparison — is the matrix's **asserted
//! verdict** (pinned by `accept_f17_d_retail_the_comparison_s_material_coverage_is_reported_not_assumed`)
//! and it is written out in
//! `docs/findings/2026-10-07-f17-d-comparison-matrix-and-material-coverage.md`,
//! which is where that state lives. The claim is `implemented`, never `checked`
//! or `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::render::matrix::{
    ComparisonSubject, MATRIX_WORLD_GROUP, MaterialCoverage, MatrixContainer, resolve_all,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

/// The acceptance tests whose capabilities this report declares.
///
/// `retail` is carried by the first two, `gpu` by the third. All three must
/// appear in the recorded log and pass, or the report is not written with that
/// capability.
const REQUIRED_TESTS: &[&str] = &[
    "accept_f17_d_retail_every_subject_resolves_to_named_original_content",
    "accept_f17_d_retail_the_comparison_s_material_coverage_is_reported_not_assumed",
    "accept_f17_d_gpu_every_subject_draws_a_measured_frame",
];

/// The synthetic half of the suite, which must be present beside the retail
/// one: an evidence report that only ever ran retail tests would not show that
/// the matrix's contract is pinned without the original data.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f17_d_the_comparison_set_is_exactly_the_five_required_subjects",
    "accept_f17_d_a_set_missing_or_duplicating_a_required_subject_is_refused",
    "accept_f17_d_an_unresolved_subject_stays_in_the_set_with_its_reason",
    "accept_f17_d_a_selection_rule_that_finds_nothing_is_refused_by_name",
    "accept_f17_d_a_selection_rule_finds_the_anchor_and_its_candidates",
    "accept_f17_d_material_coverage_counts_every_material_and_never_invents_a_class",
];

/// The derived matrix, written beside the report and referenced by digest:
/// subject, anchor, chosen mesh, refusals and coverage counts only, never
/// original content.
const MATRIX_ARTIFACT: &str = "comparison-matrix.json";

/// The per-subject GPU captures the retail GPU test wrote.
const CAPTURE_PREFIX: &str = "subject-";
const CAPTURE_SUFFIX: &str = ".png";

// ------------------------------------------------------------------ F17-E ---
//
// The same harness for task F17-E-MATRIX-COVERAGE (Rally #736): the matrix is
// widened to **every** discovered world group and each resolved world-side
// subject is drawn a second time with the materials the group's own texture
// archive binds (`cs_app::world::textured_capture`), where a texture that does
// not resolve is the `missing_texture` refusal rather than a neutral stand-in.
//
// The sequence is the F17-D one with the F17-E names:
//
// 1. ```sh
//    cargo test --workspace --locked -- accept_f17_e_ --include-ignored \
//      2>&1 | tee private/evidence/F17-E-MATRIX-COVERAGE/cargo-test.log
//    ```
// 2. ```sh
//    CS_EVIDENCE_DIR=private/evidence/F17-E-MATRIX-COVERAGE \
//    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f17_e_ --include-ignored" \
//    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//      cargo test --locked -p cs_app --test render evidence_report_f17_e -- --ignored
//    ```
// 3. ```sh
//    python3 tools/validate_evidence.py \
//      private/evidence/F17-E-MATRIX-COVERAGE/acceptance.json \
//      --artifact-root private/evidence/F17-E-MATRIX-COVERAGE --require-pass
//    ```
// 4. Commit a copy of `acceptance.json` as
//    `docs/findings/evidence/F17-E-MATRIX-COVERAGE.json`.

/// The acceptance tests whose capabilities the F17-E report declares:
/// `retail` by the first (production discovery + readers over every world
/// group), `gpu` by the second (the textured captures on a real adapter).
const REQUIRED_TESTS_F17_E: &[&str] = &[
    "accept_f17_e_retail_every_discovered_world_group_is_a_matrix_entry",
    "accept_f17_e_gpu_every_resolved_world_subject_is_captured_or_refused_by_name",
];

/// The F17-E synthetic half, which must be present beside the retail one.
const SYNTHETIC_TESTS_F17_E: &[&str] = &[
    "accept_f17_e_every_offered_group_is_a_matrix_entry_in_offered_order",
    "accept_f17_e_a_refused_airframe_container_is_reported_on_every_group",
    "accept_f17_e_each_group_resolves_against_its_own_stored_forest",
    "accept_f17_e_a_missing_texture_is_a_refusal_never_a_fallback",
    "accept_f17_e_the_capture_settings_are_the_fixed_comparison_set",
];

/// The derived widened matrix, written beside the report and referenced by
/// digest: per group, subject, anchor, chosen mesh, refusals and coverage
/// counts only, never original content.
const WIDE_MATRIX_ARTIFACT: &str = "widened-matrix.json";

/// The per-group textured captures the retail GPU test wrote.
const TEXTURED_CAPTURE_PREFIX: &str = "textured-";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f17_e_writes_the_acceptance_report() {
    use cs_app::render::matrix::{MatrixContainer, WidenedMatrix, WorldGroupSource, resolve_wide};

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

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log, "accept_f17_e_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f17_e_` tests were recorded in {}",
        log_path.display()
    );

    for required in REQUIRED_TESTS_F17_E.iter().chain(SYNTHETIC_TESTS_F17_E) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F17-E requires capabilities `retail` and `gpu`, run \
                     step 1 with `--include-ignored`, CS_GAME_DIR set and a GPU adapter available"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The widened matrix itself, resolved again over that installation: the
    // report's entries are this report's own run, not a transcription of a
    // test message.
    let sources = cs_app::playtest_retail::read_all_playtest_sources(&game_dir)
        .expect("the all-groups source read must succeed where the test ran");
    let aircraft = sources
        .aircraft
        .as_ref()
        .map(|container| {
            MatrixContainer::new(
                container.container_key(),
                container.nodes(),
                container.meshes(),
                container.materials(),
            )
        })
        .map_err(|error| error.to_string());
    let groups: Vec<WorldGroupSource> = sources
        .groups
        .iter()
        .map(|source| WorldGroupSource {
            group: source.group.clone(),
            container: source
                .container
                .as_ref()
                .map(|container| {
                    MatrixContainer::new(
                        container.container_key(),
                        container.nodes(),
                        container.meshes(),
                        container.materials(),
                    )
                })
                .map_err(|error| error.to_string()),
        })
        .collect();
    let wide: WidenedMatrix = resolve_wide(aircraft, groups);
    assert_eq!(
        wide.groups.len(),
        sources.groups.len(),
        "every discovered world group is a matrix entry"
    );
    for entry in &wide.groups {
        assert_eq!(
            entry.matrix.rows().len(),
            ComparisonSubject::ALL.len(),
            "{}: all five subjects are rows",
            entry.group
        );
    }

    let matrix_path = evidence_dir.join(WIDE_MATRIX_ARTIFACT);
    fs::write(
        &matrix_path,
        wide_matrix_json(&sources, &wide, &install_sha256),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", matrix_path.display()));

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&matrix_path, "json", &evidence_dir));
    let captures = capture_artifacts(&evidence_dir, TEXTURED_CAPTURE_PREFIX, CAPTURE_SUFFIX);
    assert!(
        !captures.is_empty(),
        "the retail GPU test must have written at least one textured capture"
    );
    artifacts.extend(captures);

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F17-E-MATRIX-COVERAGE\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"],\n\
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
        jstr(&review_identity_f17_e()),
        jstr(&review_method_f17_e()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F17-E-MATRIX-COVERAGE\"",
        "\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"]",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The widened matrix, as JSON: per group, per subject — anchors, chosen
/// meshes, skipped candidates, refusals and coverage verdicts, plus the
/// group's container and archive read outcomes. Counts, names and codes only,
/// never original content.
fn wide_matrix_json(
    sources: &cs_app::playtest_retail::PlaytestAllSources,
    wide: &cs_app::render::matrix::WidenedMatrix,
    install_sha256: &str,
) -> String {
    let mut groups = String::new();
    for (index, entry) in wide.groups.iter().enumerate() {
        if index > 0 {
            groups.push(',');
        }
        let source = &sources.groups[index];
        let container_error = entry
            .container_error
            .as_deref()
            .map_or("null".to_owned(), jstr);
        let archive = source.textures.as_ref().map_or_else(
            |error| {
                format!(
                    "{{\"resolved\":false,\"reason\":{}}}",
                    jstr(&error.to_string())
                )
            },
            |archive| {
                format!(
                    "{{\"resolved\":true,\"path\":{},\"sha256\":{},\"selection\":{}}}",
                    jstr(archive.path()),
                    jstr(archive.sha256()),
                    jstr(archive.selection()),
                )
            },
        );
        groups.push_str(&format!(
            "{{\"group\":{},\"container_error\":{},\"archive\":{},\
              \"fully_resolved\":{},\"rows\":[{}]}}",
            jstr(&entry.group),
            container_error,
            archive,
            entry.matrix.is_fully_resolved(),
            rows_json(&entry.matrix),
        ));
    }
    format!(
        "{{\"schema\":\"cs-f17-e-widened-matrix/1\",\"install_sha256\":{},\
          \"aircraft\":{},\"group_count\":{},\"groups_with_world_subjects\":{},\
          \"groups\":[{}]}}",
        jstr(install_sha256),
        sources
            .aircraft
            .as_ref()
            .map(|container| jstr(container.container_key()))
            .unwrap_or_else(|error| format!("{{\"refused\":{}}}", jstr(&error.to_string()))),
        wide.groups.len(),
        wide.groups_with_world_subjects(),
        groups,
    )
}

/// The rows of one group's matrix, as the F17-D artifact serializes them.
fn rows_json(matrix: &cs_app::render::matrix::ComparisonMatrix) -> String {
    let mut rows = String::new();
    for (index, row) in matrix.rows().iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        let subject = row.subject().code();
        let Some(resolved) = row.resolved() else {
            rows.push_str(&format!(
                "{{\"subject\":{},\"resolved\":false,\"reason\":{}}}",
                jstr(subject),
                jstr(row.reason().unwrap_or("no reason recorded"))
            ));
            continue;
        };
        let skipped = resolved
            .skipped
            .iter()
            .map(|skip| {
                format!(
                    "{{\"slot\":{},\"name\":{},\"reason\":{}}}",
                    skip.slot,
                    jstr(&skip.name),
                    jstr(&skip.reason)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        rows.push_str(&format!(
            "{{\"subject\":{},\"resolved\":true,\"container\":{},\
              \"anchor\":{{\"slot\":{},\"name\":{}}},\
              \"chosen\":{{\"slot\":{},\"name\":{},\"mesh_index\":{}}},\
              \"skipped\":[{}],\"triangles\":{},\
              \"extent\":[{},{},{}],\"unknowns\":[{}],\"coverage\":{}}}",
            jstr(subject),
            jstr(&resolved.container),
            resolved.anchor_slot,
            jstr(&resolved.anchor_name),
            resolved.chosen.slot,
            jstr(&resolved.chosen.name),
            resolved.chosen.mesh_index,
            skipped,
            resolved.triangles,
            resolved.extent[0],
            resolved.extent[1],
            resolved.extent[2],
            resolved
                .unknowns
                .iter()
                .map(|unknown| jstr(unknown.code()))
                .collect::<Vec<_>>()
                .join(","),
            coverage_json(&resolved.coverage),
        ));
    }
    rows
}

fn review_identity_f17_e() -> String {
    String::from(
        "implementer: swe2-max-1 (Devin, Rally #736 implement claim). \
         Reviewer: recorded by the reviewer in the complete_review notes — this report was \
         written by the implementer, so it is not independent evidence and no agent review \
         replaces the owner's human approval. Nothing here is verified_original: no original \
         run has happened, and `retail` in this report means read access to the owner's files \
         only",
    )
}

fn review_method_f17_e() -> String {
    String::from(
        "the acceptance suite re-run locally with the retail capability and a real GPU adapter; \
         this harness derives every field from the recorded log, production discovery of \
         $CS_GAME_DIR, and the production widened comparison matrix resolved again over that \
         installation (`cs_app::render::matrix::resolve_wide` over every discovered world group \
         from `cs_app::playtest_retail::read_all_playtest_sources`) with the textured captures \
         the retail test wrote on the real adapter \
         (`cs_app::world::textured_capture::capture_subject_textured`, which binds each material \
         group's texture out of the group's own archive through the production `TextureBinder` \
         and `WorldMeshes` path under the fixed comparison settings of spec F17 non-negotiable \
         3, and refuses with `missing_texture` — no PNG — when a named texture does not \
         resolve); validated with tools/validate_evidence.py --require-pass. The textured \
         captures bind images under the #666 declared name reading and provisional \
         presentation: they are evidence that the group's own textures resolve and draw on the \
         subject's real geometry, never a claim about the original renderer's appearance, and \
         never a comparison against an original screenshot (#358 REF-OWNER-FIRST-CAPTURE \
         remains the other side of the comparison). `claim` is `implemented` only",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f17_d_writes_the_acceptance_report() {
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
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
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
    let suite = parse_suite(&log, "accept_f17_d_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f17_d_` tests were recorded in {}",
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
                    "{required} did not run: F17-D requires capabilities `retail` and `gpu`, run \
                     step 1 with `--include-ignored`, CS_GAME_DIR set and a GPU adapter available"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery the matrix is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The matrix itself, resolved again over that installation: the report's
    // rows are this report's own run, not a transcription of a test message.
    let sources = cs_app::playtest_retail::read_playtest_sources(&game_dir, MATRIX_WORLD_GROUP)
        .expect("the world and airframe containers must read");
    let world = MatrixContainer::new(
        sources.world().container_key(),
        sources.world().nodes(),
        sources.world().meshes(),
        sources.world().materials(),
    );
    let aircraft = MatrixContainer::new(
        sources.aircraft().container_key(),
        sources.aircraft().nodes(),
        sources.aircraft().meshes(),
        sources.aircraft().materials(),
    );
    let matrix = resolve_all(&world, &aircraft);
    assert!(
        matrix.is_fully_resolved(),
        "the matrix did not resolve every subject; refusals: {:?}",
        matrix
            .rows()
            .iter()
            .filter_map(|row| row.reason().map(str::to_owned))
            .collect::<Vec<_>>()
    );

    let matrix_path = evidence_dir.join(MATRIX_ARTIFACT);
    fs::write(&matrix_path, matrix_json(&matrix, &install_sha256))
        .unwrap_or_else(|error| panic!("write {}: {error}", matrix_path.display()));

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&matrix_path, "json", &evidence_dir));
    let captures = capture_artifacts(&evidence_dir, CAPTURE_PREFIX, CAPTURE_SUFFIX);
    assert_eq!(
        captures.len(),
        ComparisonSubject::ALL.len(),
        "the retail GPU test must have written one capture per required subject"
    );
    artifacts.extend(captures);

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F17-D\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"],\n\
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
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F17-D\"",
        "\"capabilities\": [\"retail\", \"gpu\", \"synthetic\"]",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The resolved matrix, as JSON: counts, names, refusals and coverage verdicts
/// only — never a stored byte and never original content.
fn matrix_json(matrix: &cs_app::render::matrix::ComparisonMatrix, install_sha256: &str) -> String {
    format!(
        "{{\"schema\":\"cs-f17-d-comparison-matrix/1\",\"install_sha256\":{},\
          \"world_group\":{},\"subject_count\":{},\"fully_resolved\":{},\
          \"all_materials_unclassified\":{},\"rows\":[{}]}}",
        jstr(install_sha256),
        jstr(MATRIX_WORLD_GROUP),
        ComparisonSubject::ALL.len(),
        matrix.is_fully_resolved(),
        matrix
            .rows()
            .iter()
            .filter_map(|row| row.resolved())
            .all(|resolved| resolved.coverage.is_fully_unclassified()),
        rows_json(matrix),
    )
}

/// One subject's coverage verdict, as counts and refusal codes.
fn coverage_json(coverage: &MaterialCoverage) -> String {
    let reasons = coverage
        .reasons()
        .iter()
        .map(|(code, count)| format!("{}: {count}", jstr(code)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"total\":{},\"classified\":{},\"unclassified\":{},\"complete\":{},\
          \"fully_unclassified\":{},\"reasons\":{{{reasons}}}}}",
        coverage.total(),
        coverage.classified(),
        coverage.unclassified(),
        coverage.is_complete(),
        coverage.is_fully_unclassified(),
    )
}

/// Every capture PNG of one prefix the retail GPU test wrote, hashed as
/// artifacts.
fn capture_artifacts(
    evidence_dir: &Path,
    prefix: &str,
    suffix: &str,
) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = fs::read_dir(evidence_dir)
        .expect("the evidence directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix))
        })
        .map(|path| {
            let name = path
                .file_name()
                .expect("a capture has a file name")
                .to_string_lossy()
                .into_owned();
            let bytes = fs::read(&path).expect("a capture is readable");
            (name, sha256(&bytes).to_hex(), "png".to_owned())
        })
        .collect();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn review_identity() -> String {
    String::from(
        "implementer: bunny-alpha-2 (opencode, Rally #72 implement claim of 2026-10-07). \
         Reviewer: recorded by the reviewer in the complete_review notes — this report was \
         written by the implementer, so it is not independent evidence and no agent review \
         replaces the owner's human approval. Nothing here is verified_original: no original \
         run has happened, and `retail` in this report means read access to the owner's files \
         only",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with the retail capability and a real GPU adapter, \
         every `accept_f17_d_*` test alone with `--exact` as well as together; this harness \
         derives every field from the recorded log, production discovery of $CS_GAME_DIR, and the \
         production comparison matrix resolved again over that installation \
         (`cs_app::render::matrix::resolve_all`) with the GPU captures the retail test wrote on \
         the real adapter (`cs_app::world::gpu_capture::capture_world_mesh`); validated with \
         tools/validate_evidence.py --require-pass. The report's `unknowns` are this task's own \
         blockers and are empty because the acceptance run passed; the product incompleteness \
         the matrix measured — every stored material of the comparison set is `undeclared`, so no \
         original render class is established yet, and the other side of the comparison needs an \
         original run (#358 REF-OWNER-FIRST-CAPTURE) — is the matrix's asserted verdict, pinned by \
         its acceptance test and written out in \
         docs/findings/2026-10-07-f17-d-comparison-matrix-and-material-coverage.md, not dropped. \
         The captures draw stored geometry through the production upload adapter under \
         capture_world_mesh's declared flat material and key light: they are evidence that the \
         original geometry is presentable, never a claim about the original's textures, lighting \
         or colours. `claim` is `implemented` only",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/render/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F17-D` written relative to the
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the tests whose
/// name contains `prefix` (e.g. `accept_f17_d_`) from a recorded `cargo test`
/// output.
fn parse_suite(log: &str, prefix: &str) -> Suite {
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
            let full = &after[..separator];
            if !full.contains(prefix) {
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

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
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

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
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
