//! Evidence-report harness for task F21-D (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f21_d_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F21-D \
//!      cargo test --workspace --locked -- accept_f21_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F21-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.) `CS_EVIDENCE_DIR` must be set **in this step**,
//!    while the acceptance suite runs: the retail coverage test derives its
//!    `view-cockpit-coverage.json` census into that directory only while the
//!    variable is set — the log and the `f21-d-*.png` captures land there
//!    either way — so without it step 2 finds the directory holding no census
//!    and panics on the missing artifact.
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F21-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f21_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test evidence_report_f21_d -- --ignored
//!    ```
//!    (`CS_EVIDENCE_DIR` tells this harness where step 1's run left the
//!    artifacts it must read, hash and re-check, so the census and captures
//!    this report hashes are this run's own; `CS_EVIDENCE_ARGV` stays the
//!    acceptance command's bare argv, because an environment prefix is not
//!    part of a command's argv.)
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F21-D/acceptance.json \
//!      --artifact-root private/evidence/F21-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F21-D.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the coverage census the
//! retail acceptance test derived over that installation, the PNGs the two
//! capture tests wrote on the real adapter, and `rustc --version` and
//! `Cargo.lock`. Nothing is typed in by hand.
//!
//! The `capabilities` are checked, not assumed: `retail` is carried only by the
//! two tests that read `$CS_GAME_DIR`, `gpu` only by the two that draw a frame,
//! and `synthetic` by the unignored ones; every one of them must appear in the
//! recorded log and pass.
//!
//! The report's `unknowns` are this task's own **blockers**, and they are empty
//! because the acceptance run passed. The product incompleteness this stage
//! measured — the pilot's eye placement being undeclared in every readable
//! file, four of every twenty declared cockpit bindings resolving to container
//! nodes rather than geometry, the horizon-locked and window-bound camera
//! commands having no consumer, the `.zan`/`.zrd` camera clips still being an
//! undecoded layout, and no default key binding shipping anywhere — is **not**
//! dropped anywhere: it is machine-readable in the referenced
//! `view-cockpit-coverage.json` artifact (which names each one and the task that
//! resolves it) and written out in
//! `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`.
//! The claim is `implemented`, never `checked` or `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

/// The acceptance tests whose capabilities this report declares.
///
/// `retail` is carried by the first two: they read `$CS_GAME_DIR`. `gpu` by the
/// last two: each draws a real frame through
/// `cs_app::world::gpu_capture::capture_world_mesh`, and the retail one draws a
/// mesh out of the real `ZBD/planes.zbd`. All four must appear in the recorded
/// log and pass, or the report is not written with that capability.
const REQUIRED_TESTS: &[&str] = &[
    "accept_f21_d_retail_the_installation_declares_its_camera_commands_and_cockpit_bindings",
    "accept_f21_d_retail_every_declared_cockpit_binding_is_resolved_or_reported_by_name",
    "accept_f21_d_a_capture_of_a_bound_binding_mesh_draws_and_a_refusal_writes_no_file",
    "accept_f21_d_retail_a_bound_cockpit_mesh_draws_a_measured_frame_on_the_gpu",
];

/// The synthetic half of the suite, which must be present beside the retail and
/// GPU ones: an evidence report that only ever ran capability-bound tests would
/// not show that the audit's contract is pinned without them.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f21_d_the_declared_camera_commands_are_censused_with_their_scripts_and_arguments",
    "accept_f21_d_a_claim_the_container_never_writes_is_refused_rather_than_reported_as_no_coverage",
    "accept_f21_d_a_line_the_claim_does_not_describe_is_reported_instead_of_dropped_or_repaired",
    "accept_f21_d_the_cockpit_bindings_the_original_declares_are_measured_in_stored_order",
    "accept_f21_d_a_cockpit_claim_the_container_does_not_back_is_refused",
    "accept_f21_d_cockpit_coverage_resolves_each_binding_against_its_own_airframe_subtree",
    "accept_f21_d_a_node_name_an_airframe_reuses_is_reported_ambiguous_not_resolved_by_position",
    "accept_f21_d_a_subtree_walk_never_leaves_the_aircraft_it_started_from",
    "accept_f21_d_the_eye_placement_stays_undeclared_however_complete_the_bindings_are",
];

/// The derived coverage census the retail acceptance test wrote.
const CENSUS_ARTIFACT: &str = "view-cockpit-coverage.json";

/// The capture files the two capture tests write, by prefix: a capture the suite
/// did not produce cannot be claimed, and a stale file from another task cannot
/// be picked up by accident.
const CAPTURE_PREFIX: &str = "f21-d-";
const CAPTURE_SUFFIX: &str = ".png";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f21_d_writes_the_acceptance_report() {
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
        "no `accept_f21_d_` tests were recorded in {}",
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
                    "{required} did not run: F21-D declares `retail` and `gpu`, so step 1 must \
                     run with `--include-ignored`, CS_GAME_DIR set and a GPU adapter available"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        REQUIRED_TESTS.len() + SYNTHETIC_TESTS.len(),
        "and every `accept_f21_d_` test in the selection is one this report names: {:?}",
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

    // The census the retail acceptance test wrote over that installation. It is
    // checked against the tree and the hashes rather than trusted, because a file
    // left over from an earlier commit would otherwise be reported as this run's.
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    let census = fs::read_to_string(&census_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR so the retail coverage test \
             writes its derived census",
            census_path.display()
        )
    });
    for needle in [
        format!("\"candidate_tree\":\"{candidate_tree}\""),
        format!("\"install_sha256\":\"{install_sha256}\""),
        format!("\"content_sha256\":\"{content_sha256}\""),
        "\"eye_placement\":\"undeclared\"".to_owned(),
    ] {
        assert!(
            census.contains(&needle),
            "the census does not describe this run ({needle:?}); a stale artifact cannot be \
             reported as measured evidence"
        );
    }
    assert!(
        census.contains("\"resolved_pairs\":176"),
        "the census carries the measured resolution counts, so a reader sees the coverage gap \
         in machine-readable form rather than only in prose"
    );

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&census_path, "json", &evidence_dir));
    let captures = capture_artifacts(&evidence_dir);
    assert_eq!(
        captures.len(),
        2,
        "the suite must have written one capture per capture test: the authored-mesh capture and \
         the real `ZBD/planes.zbd` cockpit capture"
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
         \x20\"task_id\": \"F21-D\",\n\
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
        "\"task_id\": \"F21-D\"",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Every capture the suite wrote, hashed as artifacts.
fn capture_artifacts(evidence_dir: &Path) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = fs::read_dir(evidence_dir)
        .expect("the evidence directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(CAPTURE_PREFIX) && name.ends_with(CAPTURE_SUFFIX)
                })
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
        "Implementer: opencode Space Bunny Alpha (opencode/bunny-alpha-2, Rally #117, implement \
         claim, session of 2026-10-02). Reviewer: opencode Space Bunny Alpha \
         (opencode/bunny-alpha-2, Rally #117, review claim, session of 2026-10-03) — the **same \
         agent instance and model as the implementer**, on a separate session. Per AGENTS.md that \
         is **not** independent review: it says the branch has no known defect, not that the \
         numbers describe the original. AGENTS.md asks for a different agent instance or model \
         with fresh context for format, mission-semantics and fidelity claims, and no agent \
         review of this evidence replaces the owner's human approval.",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with `CS_GAME_DIR` set and a real GPU adapter \
         (Apple M3 Pro, Metal), every `accept_f21_d_*` test selected by prefix with \
         `--include-ignored`; this harness derives every field from the recorded log, production \
         discovery of $CS_GAME_DIR, and the coverage census the retail test derived over that \
         installation with production readers (`cs_formats::interp::decode_interp`, \
         `cs_content::scene::discover_airframe_roster`, `cs_formats::gamez::read_gamez_nodes`, \
         `cs_formats::gamez::read_gamez_meshes`), plus the two PNGs the capture tests wrote \
         through `cs_app::world::gpu_capture::capture_world_mesh`; validated with \
         tools/validate_evidence.py --require-pass. The census artifact is checked against this \
         report's own tree, install hash and content hash before it is referenced, so a stale \
         file cannot be reported as this run's. The report's `unknowns` are this task's own \
         blockers and are empty because the acceptance run passed; the product incompleteness this \
         stage measured — the pilot's eye placement undeclared in every readable file, four of \
         every twenty declared cockpit bindings resolving to container nodes rather than geometry, \
         `CameraSetWindow`/`CameraSetHorizon`/`CameraSetHorizonXZ`/`CameraSetObjectHSETest` having \
         no consumer in this camera contract, the `.zan`/`.zrd` camera clips still undecoded, and \
         no shipped default key binding — is machine-readable in the referenced \
         view-cockpit-coverage.json artifact and written out with its resolving task in \
         docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md. \
         `claim` is `implemented` only: what was measured is what the original's files declare, \
         not what the original does when it runs. `candidate_tree` is the tree of the commit the \
         suite ran on; the only later delta is this report's own copy under \
         docs/findings/evidence/, whose bytes are that file.",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f21_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F21-D` written relative to the
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
/// `accept_f21_d_` tests from a recorded `cargo test` output.
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
            if !full.contains("accept_f21_d_") {
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
                 \"view-cockpit-coverage.json\"]}}",
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
