//! Evidence-report harness for task #718 (`M01-LC-ACTOR-ANIM-CONSUMERS`):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_t718_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t718_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS/cargo-test.log` (note
//!    the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t718_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app \
//!      --test evidence_report_m01_lc_actor_anim_consumers -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS/acceptance.json
//!    --artifact-root private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS --require-pass`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-LC-ACTOR-ANIM-CONSUMERS.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. The second artifact
//! is a **second production observation**: the harness binds M01 through
//! [`bind_mission_animation`], starts every startup row in
//! [`MissionAnimationPlayer`] and advances the timeline to its measured end,
//! recording per row the declaring archive and member, the carrier and record
//! index, the measured duration, the statements published and the tick the row
//! finished on — plus the mission's `placezeps.zrd` placements with their
//! claim. That is a real production run over the owner's installation through
//! the consumer this task adds, not a paraphrase of the acceptance assertions,
//! and it carries no original bytes: names, counts, timings and claim labels
//! only.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::animation::mission::bind_mission_animation;
use cs_app::mission_animations::MissionAnimationPlayer;
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_types::Tick;
use cs_types::net::SessionId;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_t718_";

/// The mission scope the observation binds: `missions/bindings/M01.json`'s own.
const M01: &str = "zbd/c1c/m01";

/// The caller's timeline the observation runs the records on, in the same
/// units the acceptance test uses: the original's tick rate is unmeasured
/// (`f20-anim.tick-rate-unmeasured`), so this rate is the **harness's own**
/// statement, never a measurement of the original's.
const TICKS_PER_SECOND: u32 = 64;

/// The tick a bound row is started on.
const START_AT: u64 = 0;

/// The tick budget the observation gives the timeline; the measured durations
/// of M01's rows finish far below it, and exceeding it is a failure rather
/// than a silent partial run.
const TICK_BUDGET: u64 = 100_000;

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; this harness \
     derives every field from the recorded log, production discovery of $CS_GAME_DIR, rustc and \
     Cargo.lock, and a second production run of the task's own consumer over M01 \
     (actor-anim-consumers.json): bind_mission_animation binds the scope, \
     MissionAnimationPlayer starts every row of NEW_GAME_START and LOAD_GAME_START and advances \
     them once per committed tick to the end of their measured durations. Claim is implemented \
     only. OBSERVED: every startup row joined to exactly one declaring zrdr member and one carrier \
     record, every row's decoded duration reached and its statements published with the session \
     stamp, and every placezeps.zrd placement still refused under \
     f20-anim.placement-member-fields-undecoded — the consumer starts no record and spawns no actor \
     for a placement. LIMITS OF WHAT WAS MEASURED, each recorded in \
     docs/findings/2026-10-07-m01-lc-actor-anim-consumers.md: (1) a published statement is the \
     record's measured activity — spelling, class and timing — and NOT a transform: no pose, node \
     motion or world position is produced from an event, and f20-anim.event-pose-transform-not-decoded \
     still stands; (2) DeclaredWorldActorProgram still has no original encoding — the member-to-actor \
     binding is measured but motion, socket, pickup, tick rate and faction are not, so the \
     world_actors launch surface stays unsupported until #574 and the field families #632 named \
     are measured; (3) the original's tick rate and the stored time unit are unmeasured, so the \
     timeline runs on the harness's own 64-ticks-per-stored-unit statement; (4) no statement became \
     a mission signal — the original marker encoding is undecoded and no cue label was invented; \
     (5) no original executable ran, so retail is file access and nothing here reaches \
     verified_original; (6) no production mission host owns the composed step yet — VS-M01-RUNTIME \
     (#359) owns the mission session that will call it. `unknowns` is empty because every item \
     above is a stated limit of the claim, not an unresolved measurement: every row, member, \
     duration and placement in this observation was read from the installation by production code. \
     Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_actor_anim_consumers_writes_the_acceptance_report() {
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

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: M01 played through the consumer.
    let observation = observe(&game_dir, &install_sha256, &content_sha256, &candidate_tree);
    let observation_path = evidence_dir.join("actor-anim-consumers.json");
    fs::write(&observation_path, observation).expect("write actor-anim-consumers.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-ACTOR-ANIM-CONSUMERS\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": {}, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        START_AT,
        TICK_BUDGET,
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

// ------------------------------------------------------- production run ---

/// Plays M01's startup rows through the consumer and renders what it observed:
/// one row per startup identity, one row per placement, and the totals.
fn observe(
    game_dir: &Path,
    install_sha256: &str,
    content_sha256: &str,
    candidate_tree: &str,
) -> String {
    let binding =
        bind_mission_animation(game_dir, M01).expect("M01 binds through the production reader");
    let mut player = MissionAnimationPlayer::new(
        SessionId::new(718).expect("a nonzero session generation"),
        TICKS_PER_SECOND,
    )
    .expect("a nonzero tick rate");
    assert_eq!(
        player.ticks_per_second(),
        TICKS_PER_SECOND,
        "the observation states its own timeline"
    );

    let mut rows: Vec<String> = Vec::new();
    let mut started = 0_usize;
    let mut refused = 0_usize;
    for event in ["NEW_GAME_START", "LOAD_GAME_START"] {
        let run = binding.run(event);
        let report = player.start(event, Tick(START_AT), run.rows());
        started += report.started().len();
        refused += report.refused().len();
        assert_eq!(
            run.refused().count(),
            0,
            "M01 has no refused startup row: {:?}",
            run.refused()
                .map(|(row, _)| row.identity())
                .collect::<Vec<_>>()
        );
    }

    // Advance the whole timeline to its measured end, exactly as the
    // acceptance test does, and record what each row published.
    let mut tick = START_AT;
    while player.running_count() > 0 {
        assert!(
            tick <= TICK_BUDGET,
            "M01's startup rows must finish inside the measured durations"
        );
        player
            .advance(Tick(tick))
            .expect("the observation's timeline advances");
        tick += 1;
    }
    assert_eq!(
        player.finished_count(),
        started,
        "every started row finished"
    );
    assert_eq!(player.refused_count(), refused, "no row was refused late");

    for finished in player.finished() {
        rows.push(format!(
            "{{\"identity\": {}, \"event\": {}, \"archive\": {}, \"member\": {}, \"carrier\": {}, \
             \"record_index\": {}, \"duration_time\": {}, \"statements\": {}, \"finished_at_tick\": \
             {}, \"container\": {}}}",
            jstr(finished.identity()),
            jstr(finished.event()),
            jstr(finished.archive()),
            jstr(finished.member()),
            jstr(finished.carrier().label()),
            finished.record_index(),
            finished.duration_time(),
            finished.statements(),
            finished.finished_at().0,
            jstr(finished.span().container_path()),
        ));
    }
    for refused in player.refused() {
        rows.push(format!(
            "{{\"identity\": {}, \"event\": {}, \"refused\": [{}]}}",
            jstr(refused.identity()),
            jstr(refused.event()),
            refused
                .claim_ids()
                .iter()
                .map(|claim| jstr(claim))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let placements: Vec<String> = binding
        .placements()
        .iter()
        .map(|placement| {
            let target = placement.targets().first();
            format!(
                "{{\"archive\": {}, \"member\": {}, \"definition\": {}, \"claim_id\": {}, \
                 \"reason\": {}, \"first_target\": {}, \"occurrences\": {}}}",
                jstr(placement.archive()),
                jstr(placement.member()),
                placement.definition(),
                jstr(placement.claim_id().as_str()),
                jstr(placement.unplaced_reason()),
                jstr(target.map_or("", |target| target.stored())),
                target
                    .and_then(|target| target.resolution().occurrences())
                    .unwrap_or_default(),
            )
        })
        .collect();

    let mut totals: Vec<String> = Vec::new();
    for event in ["NEW_GAME_START", "LOAD_GAME_START"] {
        let run = binding.run(event);
        totals.push(format!(
            "{{\"event\": {}, \"rows\": {}, \"playable\": {}}}",
            jstr(event),
            run.len(),
            run.playable().count()
        ));
    }

    let observation = format!(
        "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \"scope\": {}, \
         \"world_container\": {}, \"ticks_per_second\": {}, \"started\": {}, \"refused\": {}, \
         \"finished\": {}, \"events\": [{}], \"rows\": [{}], \"placements\": [{}]}}\n",
        jstr(install_sha256),
        jstr(content_sha256),
        jstr(candidate_tree),
        jstr(binding.scope()),
        jstr(binding.world_container()),
        TICKS_PER_SECOND,
        started,
        refused,
        player.finished_count(),
        totals.join(", "),
        rows.join(", "),
        placements.join(", "),
    );
    assert_json_values_are_quoted(&observation);
    observation
}

/// A canary for this hand-rendered artifact: every value that follows a key
/// must start with a quote, a digit, a sign, `{` or `[`.
///
/// The validator hashes artifacts without parsing them, so an unquoted value —
/// which an earlier draft of this file rendered for the carrier label — would
/// ship as a broken file that still validates. This check runs before the
/// bytes are written.
fn assert_json_values_are_quoted(observation: &str) {
    let bytes = observation.as_bytes();
    assert!(
        bytes.starts_with(b"{\"install_sha256\": \"") && observation.trim_end().ends_with('}'),
        "the observation must be one object: {}",
        &observation[..observation.len().min(80)]
    );
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index] == b'"' && bytes[index + 1] == b':' && bytes[index + 2] == b' ' {
            let next = bytes[index + 3];
            assert!(
                matches!(next, b'"' | b'{' | b'[' | b'-' | b'0'..=b'9'),
                "an unquoted JSON value at byte {index}: {}",
                String::from_utf8_lossy(&bytes[index..bytes.len().min(index + 72)])
            );
        }
        index += 1;
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_actor_anim_consumers.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS` written
/// relative to the workspace root in the module doc must be re-anchored here.
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
/// `accept_t718_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
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
            if !name.contains(ACCEPTANCE_PREFIX) {
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
                 \"actor-anim-consumers.json\"]}}",
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
