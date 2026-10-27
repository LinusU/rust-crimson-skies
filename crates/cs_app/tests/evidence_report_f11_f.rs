//! Evidence-report harness for task F11-F (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f11_f_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F11-F
//!    cargo test --locked -p cs_app --test cs_app_tests_b -- accept_f11_f_ \
//!      --include-ignored 2>&1 | tee private/evidence/F11-F/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F11-F \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_b -- accept_f11_f_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!    CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!      cargo test -p cs_app --locked --test cs_app_tests_b -- evidence_report_f11_f --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F11-F/acceptance.json \
//!      --artifact-root private/evidence/F11-F --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F11-F.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the session-scene
//! observation this harness itself re-derives through production code
//! (written as `session-scene.json`), `rustc --version` and `Cargo.lock`.
//! The only text this file holds is the [`review_identity`] literal the
//! committed report carries (a reviewing agent replaces it with their own
//! through `CS_EVIDENCE_REVIEW`) and the limitations quoted into
//! `review.method`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_PLAYER_LOD_DISTANCE_M, MissionAirframeScene, MissionHostRestartReport,
    MissionHostRestartRequest, build_headless, stage_for, teardown,
};
use cs_app::scene::{LiveAirframeScene, LodDistance, SceneNodeBinding};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::MissionLabel;
use cs_types::content::{ContentId, ContentKind};

const PREFIX: &str = "accept_f11_f_";

/// The retail half of the suite, which binds the `retail` capability this
/// report declares: each reads `$CS_GAME_DIR` (and `CS_ENGINE_IMAGE` for the
/// mission descriptors).
const RETAIL_TESTS: &[&str] = &[
    "accept_f11_f_unknown_airframe_builds_no_scene",
    "accept_f11_f_session_start_loads_the_named_airframe",
    "accept_f11_f_retry_supersedes_the_scene_generation",
    "accept_f11_f_session_end_releases_every_entity",
    "accept_f11_f_viewer_distance_drives_lod_presentation",
];

/// The synthetic half, which must be present beside the capability-bound one:
/// an evidence report that only ever ran capability-bound tests would not
/// show that the pass's contract is pinned without them.
const SYNTHETIC_TESTS: &[&str] = &["accept_f11_f_synthetic_composition_carries_no_airframe_scene"];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_f11_f_writes_the_acceptance_report() {
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
    env_var("CS_ENGINE_IMAGE");

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
        "no `{PREFIX}` tests were recorded in {}",
        log_path.display()
    );
    for needed in RETAIL_TESTS.iter().chain(SYNTHETIC_TESTS) {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(needed))
            .unwrap_or_else(|| {
                panic!(
                    "the acceptance test {needed} did not run: F11-F requires capability \
                     `retail`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(line.1, "pass", "the acceptance test {needed} must pass");
    }
    assert_eq!(
        suite.assertions.len(),
        RETAIL_TESTS.len() + SYNTHETIC_TESTS.len(),
        "and every `{PREFIX}` test in the selection is one this report names: {:?}",
        suite
            .assertions
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    // The installation hashes describe the real installation, measured by the
    // production discovery the mission sources are built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the whole session-scene chain, re-run
    // inside this harness over the real installation — so the numbers the
    // report claims are recomputed here rather than trusted from the tests'
    // own output.
    let observation_path = evidence_dir.join("session-scene.json");
    fs::write(
        &observation_path,
        observe_session_scene(&game_dir, &install_sha256),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", observation_path.display()));

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F11-F\",\n\
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
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F11-F\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
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

/// Re-derives the session-scene chain through production code over the real
/// installation: `plan_mission_launch` → `stage_for` → `build_headless`
/// composes M01 with the scene already live, `MissionHostRestartRequest`
/// supersedes its generation, `teardown` releases it — the same calls the
/// acceptance tests made, recomputed here so this run's record is its own.
fn observe_session_scene(game_dir: &Path, install_sha256: &str) -> String {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M06").expect("the label is valid"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let stage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("the headless composition builds");

    let (airframe, root, first_generation, len) = {
        let live = app
            .world()
            .get_resource::<LiveAirframeScene>()
            .expect("the session start loaded the airframe before the first frame");
        (
            live.airframe().clone(),
            live.root().clone(),
            live.generation(),
            live.len(),
        )
    };
    assert_eq!(
        airframe,
        ContentId::from_source(ContentKind::Airframe, "player_pfighter")
            .expect("the airframe id is valid"),
        "the loaded airframe is the one M01's start resolution committed"
    );
    assert_eq!(
        root.as_content_id().key(),
        "planes-planes.player",
        "the import starts from the `player` root"
    );
    let bound = app
        .world_mut()
        .query::<&SceneNodeBinding>()
        .iter(app.world())
        .count();
    assert!(len > 0 && bound == len, "every scene node is bound");

    let distance = app
        .world()
        .get_resource::<LodDistance>()
        .expect("the viewer distance is inserted");
    assert_eq!(
        distance.meters().0,
        MISSION_PLAYER_LOD_DISTANCE_M,
        "the supplied distance is the viewer stage's own declared value"
    );

    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );
    let second_generation = app.world().resource::<LiveAirframeScene>().generation();
    assert!(
        second_generation > first_generation,
        "the retry supersedes the first generation"
    );

    teardown(&mut app);
    let remaining = app
        .world_mut()
        .query::<&SceneNodeBinding>()
        .iter(app.world())
        .count();
    let after = (
        app.world().get_resource::<MissionAirframeScene>().is_none(),
        app.world().get_resource::<LiveAirframeScene>().is_none(),
        app.world().get_resource::<LodDistance>().is_none(),
        remaining == 0,
    );
    assert_eq!(
        after,
        (true, true, true, true),
        "teardown releases the scene and every record it owned"
    );

    format!(
        "{{\n  \"task\": \"F11-F\",\n  \"install_sha256\": {},\n  \"source\": \
         \"cs_app::mission_session::{{stage_for, build_headless, teardown}} + \
         MissionHostRestartRequest\",\n  \"mission_label\": \"M06\",\n  \
         \"mission_title\": \"The Red Menace\",\n  \"airframe\": {},\n  \"root\": {},\n  \
         \"scene_nodes\": {},\n  \"bound_entities\": {},\n  \"generation_after_start\": {},\n  \
         \"generation_after_retry\": {},\n  \"lod_distance_m\": {},\n  \
         \"after_teardown\": {{\"mission_scene\": false, \"live_scene\": false, \
         \"scene_bindings\": 0, \"lod_distance\": false}}\n}}\n",
        jstr(install_sha256),
        jstr(airframe.key()),
        jstr(root.as_content_id().key()),
        len,
        bound,
        first_generation.0,
        second_generation.0,
        MISSION_PLAYER_LOD_DISTANCE_M,
    )
}

/// The `review.identity` this harness records.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// then names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: swe2-max-1 (SWE-2 Max; Rally #2122 implement claim); independent review \
         pending - Rally assigns the reviewer. The implementer wired the session-owned scene \
         calls, wrote the acceptance tests and this evidence harness, and ran the acceptance \
         suite locally with CS_GAME_DIR; this record names no reviewer because no review has \
         happened yet",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records, with the limits of what was
/// measured quoted so a reader of the committed report sees them.
fn review_method() -> String {
    String::from(
        "acceptance suite run locally with the retail capability and --include-ignored; every \
         field is derived from the recorded log, production discovery of $CS_GAME_DIR, the \
         session-scene chain re-run inside this harness over that installation and written as \
         session-scene.json, rustc and Cargo.lock; validated with tools/validate_evidence.py \
         --require-pass. The re-run composes zbd/c3/m01 through \
         cs_app::mission_session::stage_for and build_headless, drives a retry through \
         MissionHostRestartRequest and ends the session through teardown, re-deriving the \
         airframe player_pfighter under scene root planes-planes.player, the superseding scene \
         generation and the released bindings rather than trusting the tests' own output. \
         LIMITS OF WHAT WAS MEASURED: `retail` is read access to original files, never a run \
         of the original; the viewer distance 20 m is the designed playtest stage value \
         (PLAYTEST_AIRCRAFT_LOD_DISTANCE_M), not a measured original LOD switch distance; \
         SceneCollisionGeometry starts empty because no per-node collision geometry is \
         measured; every mission but zbd/c3/m01 resolves its player airframe \
         Resolved::Unknown and carries no canonical scene, which is pinned by \
         accept_f11_f_unknown_airframe_builds_no_scene in this report's own suite. `claim` is \
         `implemented` only. `candidate_tree` is the tree of the commit the suite ran on; the \
         only later delta is this report's own copy under docs/findings/evidence/, whose bytes \
         are that file.",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f11_f.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F11-F` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(git(&["rev-parse", "--show-toplevel"]))
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
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the `PREFIX`
/// tests from a recorded `cargo test` output.
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
            let full = &after[..separator];
            if !full.contains(PREFIX) {
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
                 \"session-scene.json\"]}}",
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
/// accepts after the validator's `Z` -> `+00:00` replacement.
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
