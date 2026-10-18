//! Evidence-report harness for task #1318 (`docs/contracts/CLI-EVIDENCE.md`,
//! `schemas/evidence.schema.json`). Not named `accept_t1318_*`: it is not part of
//! the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t1318_ --include-ignored 2>&1 |
//!    tee private/evidence/T1318/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T1318 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t1318_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test cs_content_tests evidence_report_t1318 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T1318/acceptance.json
//!    --artifact-root private/evidence/T1318 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T1318.json`.
//!
//! `blast-unknowns.json` is a second production observation: every retail
//! rocket record's blast-relevant numbers as `read_original_rocket_blasts`
//! returns them (now including the `RANGE_MINIMUM` arming pair), the
//! executable's SHA-256, and the resolution of each unknown task #548 left
//! open. Identifiers, numbers and digests only; no original display text.
//!
//! **On `unknowns`.** `--require-pass` rejects a nonempty `unknowns` list, and
//! this task has no unresolved issue inside its own assertions. The original
//! facts it leaves unmeasured are fidelity limitations, named with their
//! affected content in `LIMITATIONS` below, in `blast-unknowns.json` (a
//! hashed artifact) and in
//! `docs/findings/2026-10-18-t1318-blast-unknowns-measured.md`,
//! so none is removed from machine-readable evidence to turn a validator green.
//! The claim is `implemented`; `retail` is read access, not an original run.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_content::ordnance::read_original_rocket_blasts;

const RETAIL_TESTS: [&str; 2] = [
    "accept_t1318_retail_blast_unknowns::accept_t1318_retail_range_minimum_reads_the_arming_pair",
    "accept_t1318_retail_blast_unknowns::accept_t1318_image_pins_the_blast_unknowns",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR and CS_ENGINE_IMAGE set); this report is derived from the recorded log plus a second production pass recorded in blast-unknowns.json. MEASURED (task #548's open list, resolved by static analysis of the owner-supplied decrypted executable, sha-256 recorded): the unit scale - IMPACT_PROXIMITY, DETONATION_DISTANCE and RANGE_MINIMUM[0] reach the world-space distance comparisons with no conversion, and the world unit is the metre (F16-E landmarks); the projectile +0x678 multiplier - a blast-and-damage scale, spawned at 1.0 and multiplied by RANGE_MINIMUM[1] for a detonation inside the arming window (a zero second value suppresses the detonation outright - the torpedo's arming range); the apply loop's recipient gate - object +0xbc is an exclusion key the impact path passes as the hit object's damage group (detonations pass 0) and global 0xa1d7d4 is a re-entrancy guard cleared around nested impact processing; the area query's entry value - the centre-to-bounding-surface distance clamped at zero, squared (compared against radius^2 and stored at entry +0x1c); range-end removal - the step clears the projectile's +0x20 active word and the sweep unlinks and frees it; the special keys - a second parser fills a record at weapon +0x210 (SONIC 0x200, FLASH 0x800, BEEPER 0x4000 with TIME default 5.0, BEEPER_SEEKER 0x8000, SMOKE_SCREEN 0x10000 with TIME default 5.0, TANGLER 0x40000 with TIME/RADIUS and a tick callback at weapon +0x20c), and RANGE_MINIMUM's 0x4cd210 call toggles node flag bit 0x10. cs_content::ordnance::OriginalRocketBlast now carries the arming pair; cs_sim documents the measured surface-distance falloff and the impact exclusion. STILL UNMEASURED (none claimed): see the limitations array of blast-unknowns.json - the exact gameplay consumers of SONIC, FLASH, BEEPER and BEEPER_SEEKER (t1318.limit.status_consumers), the domain/writers of the +0xbc key (t1318.limit.exclusion_domain), and route_detonation_area still not wired into step_ordnance_session (t1318.limit.session_wiring). The claim is implemented: this awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

const LIMITATIONS: [(&str, &str, &str, &str); 3] = [
    (
        "t1318.limit.status_consumers",
        "the gameplay consumers of the SONIC, FLASH, BEEPER and BEEPER_SEEKER record bits: the flag-word stores are pinned, but no code site testing those bits was located",
        "the choke, stall and mark statuses F28 calls non-negotiable",
        "Rally #1601",
    ),
    (
        "t1318.limit.exclusion_domain",
        "the domain of the +0xbc exclusion key: the apply loop's compare and the impact path's hit-object argument are pinned, but the field's writers were not fully traced, so which objects share a key is unmeasured",
        "which objects a blast excludes beyond the directly-hit one",
        "Rally #1602",
    ),
    (
        "t1318.limit.session_wiring",
        "route_detonation_area is defined and tested in cs_sim but step_ordnance_session does not call it: the session holds no actor list to supply recipients",
        "a blast in a live session still damages only the engagement's target",
        "Rally #1603",
    ),
];

/// Each unknown task #548 left open and how this task resolved it; the byte
/// windows are pinned in `accept_t1318_image_pins_the_blast_unknowns`.
const RESOLUTIONS: [(&str, &str, &str); 7] = [
    (
        "t548.limit.units",
        "measured",
        "IMPACT_PROXIMITY, DETONATION_DISTANCE and RANGE_MINIMUM[0] are stored raw against world-space distances with no conversion; the world unit is the metre (F16-E). Pins at 0x5add70 and 0x5adbb2.",
    ),
    (
        "t548.limit.scale",
        "measured",
        "projectile +0x678 is a blast-and-damage scale: spawned at 1.0 from global 0xa1e1b0 (pins 0x5af092, 0x5af0c3), multiplied by RANGE_MINIMUM[1] for a detonation inside the arming window - [1] == 0 suppresses it outright (pins 0x5b06c6, 0x5b0668) - and into both damage channels in the apply loop (pin 0x5acb56).",
    ),
    (
        "t548.limit.recipient_gate",
        "measured",
        "object +0xbc is an exclusion key compared in the apply loop (pin 0x5acb0f); the impact path passes the hit object's damage group (pin 0x5b0741), detonations pass 0. Global 0xa1d7d4 is a re-entrancy guard (pins 0x5ac415, 0x5b09f8, 0x5b0a0b).",
    ),
    (
        "t548.limit.entry_value",
        "measured",
        "the entry value is the centre-to-bounding-surface distance clamped at zero, squared (pins 0x4cb70d, 0x4cb72d, 0x4cb73b), stored at entry +0x1c (pin 0x4cb80a).",
    ),
    (
        "t548.limit.range_end",
        "measured",
        "the step clears the projectile's +0x20 active word past RANGE (pin 0x5b0318); the sweep unlinks a dead projectile and frees it via 0x5b08d0 (pin 0x5af917).",
    ),
    (
        "t548.limit.status_keys",
        "partially measured",
        "a second parser fills a 0x38-byte record at weapon +0x210: SONIC 0x200, FLASH 0x800, BEEPER 0x4000 + TIME default 5.0, BEEPER_SEEKER 0x8000, SMOKE_SCREEN 0x10000 + TIME default 5.0, TANGLER 0x40000 + TIME/RADIUS + tick callback at weapon +0x20c (pins 0x4ba738-0x4ba859, 0x5aec98). Consumers pinned for SMOKE_SCREEN (step, 0x5af98b) and TANGLER (impact path, 0x5ac850); SONIC, FLASH, BEEPER and BEEPER_SEEKER consumers stay unmeasured (t1318.limit.status_consumers).",
    ),
    (
        "t548.limit.range_minimum",
        "measured",
        "0x4cd210 sets or clears node flag bit 0x10 at +0x24 (pin 0x4cd245); the RANGE_MINIMUM detonation path clears the projectile node's flag before the area query and restores it after, so the flag gates whether the node participates in the query.",
    ),
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t1318_writes_the_acceptance_report() {
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
    let image_path = PathBuf::from(env_var("CS_ENGINE_IMAGE"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit; a stale report cannot be \
         reused for new code"
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
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: #1318 requires capability `retail`, run step 1 \
                     with --include-ignored, CS_GAME_DIR and CS_ENGINE_IMAGE set"
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = install::discover(&game_dir)
        .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    // Second observation: the same production reader, run again.
    let rockets = read_original_rocket_blasts(&game_dir).expect("the retail records read");
    assert_eq!(rockets.len(), 15, "fifteen retail records carry ROCKET");
    let image = fs::read(&image_path).expect("the engine image reads");
    let image_sha256 = sha256(&image).to_hex();

    let number = |value: Option<f64>| value.map_or_else(|| "null".to_owned(), |n| n.to_string());
    let rocket_rows: Vec<String> = rockets
        .iter()
        .map(|rocket| {
            format!(
                "{{\"record\": {}, \"impact_proximity\": {}, \"detonation_distance\": {}, \
                 \"detonation_time_s\": {}, \"armor_damage\": {}, \"health_damage\": {}, \
                 \"lock_on\": {}, \"range_minimum\": {}, \
                 \"range_minimum_blast_scale\": {}, \"keys\": {}}}",
                jstr(&rocket.record),
                number(rocket.impact_proximity),
                number(rocket.detonation_distance),
                number(rocket.detonation_time_s),
                number(rocket.armor_damage),
                number(rocket.health_damage),
                number(rocket.lock_on),
                number(rocket.range_minimum),
                number(rocket.range_minimum_blast_scale),
                str_array(&rocket.keys.iter().cloned().collect::<Vec<String>>()),
            )
        })
        .collect();
    let limitations: Vec<String> = LIMITATIONS
        .iter()
        .map(|(claim, behavior, gates, resolving)| {
            format!(
                "{{\"claim\": {}, \"unmeasured\": {}, \"gates\": {}, \"resolving_task\": {}}}",
                jstr(claim),
                jstr(behavior),
                jstr(gates),
                jstr(resolving)
            )
        })
        .collect();
    let resolutions: Vec<String> = RESOLUTIONS
        .iter()
        .map(|(claim, status, note)| {
            format!(
                "{{\"claim\": {}, \"status\": {}, \"note\": {}}}",
                jstr(claim),
                jstr(status),
                jstr(note)
            )
        })
        .collect();
    let surface_path = evidence_dir.join("blast-unknowns.json");
    fs::write(
        &surface_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"weapons_member\": \"ZBD/zrdr.zbd weapons.zrd\", \"engine_image_sha256\": {}, \
             \"rockets\": [{}], \"resolutions\": [{}], \"limitations\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(&image_sha256),
            rocket_rows.join(", "),
            resolutions.join(", "),
            limitations.join(", "),
        ),
    )
    .expect("write blast-unknowns.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&surface_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T1318\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \
         \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \
         \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_t1318.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T1318` written relative to the
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
/// `accept_t1318_` tests from a recorded `cargo test` output.
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
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_t1318_") {
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
                 \"blast-unknowns.json\"]}}",
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
