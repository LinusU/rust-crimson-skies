//! Evidence-report harness for task F20-D (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f20_d_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_f20_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F20-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F20-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f20_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_GAME_DIR=<original installation> \
//!      cargo test --locked -p cs_app --test evidence_report_f20_d -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F20-D/acceptance.json \
//!      --artifact-root private/evidence/F20-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F20-D.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the production
//! animation-family survey run over it, the PNGs the GPU captures wrote, and
//! `rustc --version` and `Cargo.lock`. Nothing is typed in by hand.
//!
//! The `capabilities` are checked, not assumed: `retail` and `gpu` are declared
//! only because every test listed in [`CAPABILITY_TESTS`] — the two that read
//! `$CS_GAME_DIR` and the three that need an adapter — is in the recorded log
//! and passed.
//!
//! The report's `unknowns` are *this task's* blockers and are empty because the
//! acceptance run passed. The product-incompleteness state the survey measured
//! — the animation containers' undecoded payload layout, the unpaired root
//! reader members — is written out in
//! `docs/findings/2026-10-02-f20-d-animation-family-validation.md` and in the
//! `animation-families.json` census artifact, not dropped.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::animation::{CarrierKind, survey_animation_families};
use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// The acceptance tests whose capabilities this report declares.
///
/// `retail` is carried by the first two — they read `$CS_GAME_DIR` — and `gpu`
/// by the last three — the captures need a real adapter (the retail one needs
/// both). All must appear in the recorded log and pass, or the report is not
/// written with those capabilities.
const CAPABILITY_TESTS: &[&str] = &[
    "accept_f20_d_retail_every_animation_carrier_is_present_validated_and_fingerprinted",
    "accept_f20_d_retail_every_declared_campaign_mission_carries_its_animation",
    "accept_f20_d_a_clip_evaluated_pose_reaches_a_distinct_rendered_frame",
    "accept_f20_d_a_pose_draws_its_orientation_even_when_the_bounds_are_unchanged",
    "accept_f20_d_retail_geometry_driven_by_a_playing_clip_draws_two_distinct_frames",
];

/// The synthetic half of the suite, which must be present beside the retail
/// one: the AC04 skip scenario, the marker-ordering and loop rules, the
/// capture's own refusals and the survey's blocker contract — all of which
/// must pin without original data.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f20_d_skipping_a_mission_marked_clip_reaches_its_final_state_exactly_once",
    "accept_f20_d_skipping_past_several_markers_fires_each_once_in_tick_order",
    "accept_f20_d_a_looping_clip_skipped_across_passes_fires_its_oneshot_marker_once",
    "accept_f20_d_a_pose_that_does_not_fit_the_render_affine_is_refused",
    "accept_f20_d_an_empty_mesh_is_refused_rather_than_captured",
    "accept_f20_d_survey_of_a_synthetic_installation_validates_every_carrier",
    "accept_f20_d_a_scope_missing_its_animation_carrier_is_a_named_blocker",
    "accept_f20_d_a_carrier_with_a_wrong_header_is_a_named_blocker",
    "accept_f20_d_a_reader_without_the_paired_member_is_a_named_blocker",
    "accept_f20_d_a_carrier_without_a_sibling_reader_is_a_named_blocker",
];

/// The derived animation-family census, written beside the report and
/// referenced by digest: keys, versions, member names, spans and digests
/// only — never original content.
const CENSUS_ARTIFACT: &str = "animation-families.json";

/// The posed GPU captures the suite wrote, identified by prefix. The artifact
/// list is derived from what is on disk, so a capture the tests did not
/// produce cannot be claimed.
const CAPTURE_PREFIX: &str = "f20d-";
const CAPTURE_SUFFIX: &str = ".png";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f20_d_writes_the_acceptance_report() {
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
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f20_d_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in CAPABILITY_TESTS.iter().chain(SYNTHETIC_TESTS) {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F20-D requires capabilities `retail` and `gpu`, run \
                     step 1 with `--include-ignored`, CS_GAME_DIR set and a GPU adapter available"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's survey is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The survey itself, re-run over that installation: the census artifact's
    // numbers are this report's own run, not a transcription of a test
    // message.
    let survey = survey_animation_families(&game_dir)
        .expect("the production animation-family survey runs over the installation");
    let census_path = evidence_dir.join(CENSUS_ARTIFACT);
    fs::write(&census_path, census_json(&survey))
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&census_path, "json", &evidence_dir));
    artifacts.extend(capture_artifacts(&evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F20-D\",\n\
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
        "\"task_id\": \"F20-D\"",
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
    assert!(
        survey.is_clean(),
        "the survey reported a blocked or stray carrier: the report was written honestly and \
         the blocker must be fixed first"
    );
    println!("wrote {}", out.display());
}

/// The derived census of every surveyed animation scope, as JSON: keys,
/// kinds, dispatch bases, versions, sizes, digests and member names/spans —
/// never original bytes.
fn census_json(survey: &cs_app::animation::AnimationFamilySurvey) -> String {
    let mut rows = String::new();
    for (index, scope) in survey.scopes.iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        let state = match &scope.state {
            Ok(carrier) => {
                let members = carrier
                    .animation_members
                    .iter()
                    .map(|member| {
                        format!(
                            "{{\"name\":{},\"offset\":{},\"length\":{},\"sha256\":{}}}",
                            jstr(&member.name),
                            member.span.offset,
                            member.span.length,
                            jstr(&member.sha256.to_hex()),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"container\":{},\"kind\":{},\"basis\":{},\"version\":{},\
                      \"size_bytes\":{},\"sha256\":{},\"reader\":{},\"animation_members\":[{}]}}",
                    jstr(&carrier.container_key),
                    jstr(carrier.kind.label()),
                    jstr(match carrier.basis {
                        cs_formats::zbd::DispatchBasis::HeaderOnly => "header",
                        cs_formats::zbd::DispatchBasis::HeaderAndRole => "header+role",
                        cs_formats::zbd::DispatchBasis::RoleOnly => "role",
                    }),
                    carrier.version,
                    carrier.size_bytes,
                    jstr(&carrier.sha256.to_hex()),
                    jstr(&carrier.reader_key),
                    members,
                )
            }
            Err(blocker) => format!(
                "{{\"state\":\"blocked\",\"blocker\":{},\"detail\":{}}}",
                jstr(blocker.label()),
                jstr(&blocker.to_string()),
            ),
        };
        rows.push_str(&format!(
            "{{\"scope\":{},\"kind\":{},\"carrier\":{}}}",
            jstr(&scope.scope),
            jstr(scope.kind.label()),
            state,
        ));
    }
    let strays = survey
        .strays
        .iter()
        .map(|key| jstr(key))
        .collect::<Vec<_>>()
        .join(",");
    let unpaired = survey
        .unpaired_members
        .iter()
        .map(|unpaired| {
            format!(
                "{{\"reader\":{},\"name\":{},\"offset\":{},\"length\":{},\"sha256\":{}}}",
                jstr(&unpaired.reader_key),
                jstr(&unpaired.member.name),
                unpaired.member.span.offset,
                unpaired.member.span.length,
                jstr(&unpaired.member.sha256.to_hex()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let families = survey
        .payload_families()
        .iter()
        .map(|family| {
            let carriers = family
                .carriers
                .iter()
                .map(|key| jstr(key))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"sha256\":{},\"carriers\":[{}]}}",
                jstr(&family.sha256.to_hex()),
                carriers,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mission = survey.scopes_of(CarrierKind::Mission).count();
    let camera = survey.scopes_of(CarrierKind::Camera).count();
    format!(
        "{{\"schema\":\"cs-f20-d-animation-family-census/1\",\"mission_scopes\":{mission},\
          \"camera_scopes\":{camera},\"validated_carriers\":{},\"clean\":{},\"strays\":[{}],\
          \"unpaired_members\":[{}],\"payload_families\":[{}],\"scopes\":[{}]}}",
        survey.carriers().count(),
        survey.is_clean(),
        strays,
        unpaired,
        families,
        rows,
    )
}

/// Every `f20d-*.png` the GPU tests wrote, hashed as artifacts.
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
        "implementer: devin-1 (Rally claim clm_8z3ovohjd9t0dksr, SWE-2 High, session of \
         2026-10-02); reviewer: deepseek-1 (Rally claim clm_mjrv4ct0axwqor1j, DeepSeek V4.1 \
         Flash, session of 2026-10-02, fresh context — a different agent and model from the \
         implementer). The reviewer found and fixed a captured-pose defect (the pose was \
         applied as a bare `GlobalTransform` that `Mesh3d`'s required default `Transform` and \
         Bevy's transform propagation overwrote, so the capture drew the mesh at the origin) \
         and added a discriminating orientation test; an agent review is not independent \
         original-reference evidence and no agent review replaces the owner's human approval",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite run locally with the retail capability and a real GPU adapter \
         (Apple M3 Pro, Metal on the reviewer's machine; the implementer's original run was on \
         an Apple M1 Max, Metal): `cargo test --workspace --locked -- accept_f20_d_ \
         --include-ignored`, every `accept_f20_d_*` test passing — run by the implementer and \
         re-run by the reviewer on this commit after rebasing onto origin/main and fixing the \
         capture defect; this harness derives every field from the recorded log, production \
         discovery of $CS_GAME_DIR, and the production animation-family survey run over that \
         installation (`cs_app::animation::survey_animation_families`) with the posed GPU \
         captures the suite wrote on the real adapter \
         (`cs_app::animation::capture_animated_pose`); validated with \
         tools/validate_evidence.py --require-pass. The animation container payloads remain \
         undecoded — the survey validates signatures, versions, layouts, member pairings and \
         fingerprints only, which is what the findings record",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f20_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F20-D` written relative to the
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

fn sha256(bytes: &[u8]) -> cs_types::evidence::ContentHash {
    cs_assets::install::sha256(bytes)
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
/// `accept_f20_d_` tests from a recorded `cargo test` output.
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
            let full = &after[..separator];
            if !full.contains("accept_f20_d_") {
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
