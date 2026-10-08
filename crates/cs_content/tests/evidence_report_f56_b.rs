//! Evidence-report harness for task F56-B (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`). Not named `accept_f56_b_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f56_b_ --include-ignored 2>&1 |
//!    tee private/evidence/F56-B/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F56-B \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f56_b_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f56_b -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F56-B/acceptance.json
//!    --artifact-root private/evidence/F56-B --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F56-B.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. `objectives.json` is
//! a second production observation: the decoded `targets.zrd` records of all
//! 21 slots (counts, `ObjectiveKind` labels and directive key spellings only —
//! no original text) plus, for each slot that declares a possessable flag, a
//! production `ObjectiveBoard` run of the acceptance scenario (two claims on
//! one tick; one `Possessed`, one `AlreadyHeld`).

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::multiplayer::{ObjectiveKind, discover_slots};
use cs_sim::multiplayer::objective::{
    ObjectiveAction, ObjectiveBoard, ObjectiveEvent, Transition, Verdict,
};
use cs_sim::multiplayer::result::Roster;
use cs_types::Tick;
use cs_types::net::{EventId, PeerId, SessionId};

const RETAIL_TESTS: [&str; 1] =
    ["accept_f56_b_retail_every_slot_exposes_decoded_objective_records"];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; the report is derived from the recorded log, production discovery of $CS_GAME_DIR and a second production run of discover_slots recorded in objectives.json (per-slot objective counts, ObjectiveKind labels and directive key spellings — no original text). Claim is implemented only: the possession machine's states, transitions and event-id adjudication are documented engine design (docs/findings/2026-10-08-f56-b-objective-ownership.md), not measured original behavior. LIMITS OF WHAT WAS MEASURED: (1) every per-mode rule value — spawn, respawn, lives, time/score limits, friendly fire, victory/draw, disconnect, late join, human scaling, component limit — is still Resolved::Unknown; the new spawn/victory RuleFields have vocabularies but no measured values, so no mode can yet resolve; (2) the original's possession, drop, return, delivery and point rules are unknown; the board's arbitration order is the engine's documented design; (3) which deathmatch variant a slot launches under is unresolved; (4) net.zrd's ~40 four-float records are a spawn-placement candidate whose semantics are unmeasured; (5) only the retail string/installation bytes were read — no original run is evidence of behavior. `unknowns` is empty because the implementation and the retail decode itself have nothing unresolved; these limits gate every fidelity or release claim. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f56_b_writes_the_acceptance_report() {
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
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
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
    // and labels only (record key spellings, never original text).
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
            let cs_types::content::Resolved::Known(objectives) = &slot.objectives else {
                panic!("{} must resolve its records", slot.id);
            };
            let count = |kind: ObjectiveKind| {
                objectives
                    .value
                    .iter()
                    .filter(|objective| objective.kind() == kind)
                    .count()
            };
            let possessable = objectives
                .value
                .iter()
                .filter(|objective| objective.possessable())
                .count();
            let mut directives: Vec<String> = objectives
                .value
                .iter()
                .flat_map(|objective| objective.directives.iter().cloned())
                .collect();
            directives.sort();
            directives.dedup();
            format!(
                "{{\"id\": {}, \"mode\": {}, \"objectives\": {}, \"flag\": {}, \"flag_base\": {}, \"zeppelin_enemy\": {}, \"zeppelin_friend\": {}, \"rearm_base\": {}, \"other_kind\": {}, \"possessable\": {}, \"directives\": {}, \"possession\": {}}}",
                jstr(slot.id.as_str()),
                jstr(mode),
                objectives.value.len(),
                count(ObjectiveKind::Flag),
                count(ObjectiveKind::FlagBase),
                count(ObjectiveKind::ZeppelinEnemy),
                count(ObjectiveKind::ZeppelinFriend),
                count(ObjectiveKind::RearmBase),
                count(ObjectiveKind::Other),
                possessable,
                str_array(&directives),
                possession_run(possessable),
            )
        })
        .collect();
    let objectives_path = evidence_dir.join("objectives.json");
    fs::write(
        &objectives_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"slots\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            slot_items.join(", ")
        ),
    )
    .expect("write objectives.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&objectives_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F56-B\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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

/// The acceptance scenario run on a board sized like the slot declares: two
/// peers claim the first objective on one tick; the record is `"applied:N,
/// held:peer"` or `-"` for a slot with nothing possessable.
fn possession_run(possessable: usize) -> String {
    if possessable == 0 {
        return jstr("-");
    }
    let roster = Roster::free_for_all(&[PeerId::new(1).unwrap(), PeerId::new(2).unwrap()])
        .expect("a two-peer roster");
    let mut board = ObjectiveBoard::new(SessionId::new(1).unwrap(), &roster);
    let objectives: Vec<_> = (0..possessable).map(|_| board.declare()).collect();
    let claim = |producer: u32, sequence: u32, claimant: u16| ObjectiveEvent {
        id: EventId {
            session: SessionId::new(1).unwrap(),
            tick: Tick(10),
            producer,
            sequence,
        },
        objective: objectives[0],
        action: ObjectiveAction::Claim {
            claimant: PeerId::new(claimant).unwrap(),
        },
    };
    // The later event id is submitted first: adjudication follows the ids.
    board.submit(claim(2, 0, 2)).expect("queued");
    board.submit(claim(1, 0, 1)).expect("queued");
    let rulings = board.close_tick(Tick(10));
    let mut applied = 0_u32;
    let mut denied = 0_u32;
    for ruling in &rulings {
        match ruling.verdict {
            Verdict::Applied(Transition::Possessed { .. }) => applied += 1,
            Verdict::Denied(_) => denied += 1,
            _ => panic!("a claim cannot produce another transition"),
        }
    }
    assert_eq!((applied, denied), (1, 1), "one claim holds, one is refused");
    let holder = board
        .holder(objectives[0])
        .map(|peer| peer.get().to_string())
        .expect("the accepted claim holds it");
    jstr(&format!(
        "possessed:{applied} denied:{denied} holder:{holder}"
    ))
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f56_b.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F56-B` written relative to the
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

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f56_b_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f56_b_") {
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
                 \"objectives.json\"]}}",
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
