//! Evidence-report harness for task F56-C (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`). Not named `accept_f56_c_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f56_c_ --include-ignored 2>&1 |
//!    tee private/evidence/F56-C/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F56-C \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f56_c_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f56_c -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F56-C/acceptance.json
//!    --artifact-root private/evidence/F56-C --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F56-C.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. `launches.json` is a
//! second production observation: for every slot the installation ships, the
//! map's objective count (`ScenarioSlot::possessable_objectives`) and a
//! production `MatchSession` run of the acceptance scenario — start, play,
//! restart — recorded as counts and status only (no original text).

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::multiplayer::discover_slots;
use cs_sim::multiplayer::objective::ObjectiveState;
use cs_sim::multiplayer::result::{
    LethalEvent, LethalKind, Limits, Roster, ScoreTable, Side, VictoryRule,
};
use cs_sim::multiplayer::session::{MatchSession, SessionConfig, SessionError};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{EventId, PeerId, SessionId};

const RETAIL_TESTS: [&str; 1] =
    ["accept_f56_c_retail_every_shipped_map_declares_its_objectives_and_restarts_cleanly"];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; the report is derived from the recorded log, production discovery of $CS_GAME_DIR and a second production run over every shipped slot recorded in launches.json (per-slot objective counts and a start/restart session status — no original text). Claim is implemented only: the match session's start/restart/teardown discipline, the launch pairing checks, the wire-label victory bridge and the end-of-match keys are documented engine design (docs/findings/2026-10-08-f56-c-match-wiring.md), not measured original behavior. LIMITS OF WHAT WAS MEASURED: (1) every per-mode rule value and the score table's points are still Resolved::Unknown, so no mode resolves in production and the acceptance tests supply authored inputs explicitly labeled as test inputs; (2) what a flag delivery scores and how the original presented or restarted a match are unknown; (3) the lobby's launch call site and the app's results screen live outside this task's owner paths and are unfilled (follow-up #799); (4) only retail string/archive bytes were read — no original run is evidence of behavior, and no real-client network integration (F56-D) is claimed. `unknowns` is empty because the implementation and the retail decode itself have nothing unresolved; these limits gate every fidelity or release claim. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f56_c_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE is the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail_test} did not run: run with --include-ignored"));
        assert_eq!(status, "pass", "{retail_test}");
    }

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let install_sha256 = install.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Second observation: the production slot discovery, rendered as counts
    // and statuses only (never original text), each map run through a
    // production `MatchSession` start and restart.
    let slots = discover_slots(install, &found.manifest.files, |record| {
        fs::read(game_dir.join(record.relative_spelling.as_str()))
    })
    .expect("the slot archives read");
    assert_eq!(slots.slots.len(), 21);
    let slot_items: Vec<String> = slots
        .slots
        .iter()
        .map(|slot| {
            let mode = match slot.mode.clone().known() {
                Some(mode) => mode.label(),
                None => "unknown",
            };
            let possessable = slot
                .possessable_objectives()
                .unwrap_or_else(|| panic!("{} must decode its records", slot.id));
            format!(
                "{{\"id\": {}, \"mode\": {}, \"possessable\": {}, \"launch\": {}}}",
                jstr(slot.id.as_str()),
                jstr(mode),
                possessable,
                launch_run(&slot.id, possessable),
            )
        })
        .collect();
    let launches_path = evidence_dir.join("launches.json");
    fs::write(
        &launches_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"slots\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            slot_items.join(", ")
        ),
    )
    .expect("write launches.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&launches_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F56-C\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------------- second observation ---

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn generation(number: u64) -> SessionId {
    SessionId::new(number).expect("a nonzero session number")
}

/// One production run of the acceptance scenario on one shipped map: start a
/// session declaring that map's objectives, then restart it under a new
/// generation and check that no score, pickup, timer or result came across
/// and that the finished generation's packet is refused. The record is
/// `"started:N restarted:clean"`.
fn launch_run(scenario: &ContentId, possessable: usize) -> String {
    let config = |session: SessionId| SessionConfig {
        scenario: scenario.clone(),
        session,
        roster: Roster::free_for_all(&[peer(1), peer(2)]).expect("a two-peer roster"),
        table: ScoreTable {
            kill: 1,
            crash: 1,
            team_kill: -1,
        },
        limits: Limits {
            time_limit: Some(Tick(600)),
            score_limit: Some(2),
        },
        victory: VictoryRule::HighestScore,
        objectives: possessable as u32,
    };
    let mut played = MatchSession::start(config(generation(1))).expect("the map starts a match");
    assert_eq!(played.scenario(), *scenario, "the session carries the map");
    assert_eq!(
        played.objectives(),
        possessable as u32,
        "the session declares the map's objectives"
    );
    assert_eq!(
        played.objective_ids().count(),
        possessable,
        "the board holds exactly the declared objectives"
    );

    played
        .restart(config(generation(2)))
        .expect("the map restarts a match");
    assert_eq!(played.clock(), None, "no timer leaked");
    assert_eq!(played.result(), None, "no result leaked");
    assert_eq!(played.end_of_match(), None, "no end-of-match record leaked");
    for side in [Side::Participant(peer(1)), Side::Participant(peer(2))] {
        assert_eq!(played.score(side), Some(0), "no score leaked");
    }
    for objective in played.objective_ids() {
        assert_eq!(
            played.objective_state(objective),
            Some(ObjectiveState::Home),
            "no pickup leaked"
        );
        assert_eq!(played.captures_of(objective).map(<[_]>::len), Some(0));
    }
    // The previous generation's packet is refused, not inherited.
    let refused = played.submit_lethal(LethalEvent {
        id: EventId {
            session: generation(1),
            tick: Tick(5),
            producer: 0,
            sequence: 0,
        },
        victim: peer(1),
        kind: LethalKind::Crash,
    });
    assert!(
        matches!(
            refused,
            Err(SessionError::Lethal(
                cs_sim::multiplayer::result::SubmitError::WrongSession { got }
            )) if got == generation(1)
        ),
        "the old generation's event must be refused"
    );
    jstr(&format!("started:{possessable} restarted:clean"))
}

// ----------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f56_c.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F56-C` written relative to the
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

// ------------------------------------------------------------ log parsing ---

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
/// `accept_f56_c_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f56_c_") {
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

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
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
                 \"launches.json\"]}}",
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
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
