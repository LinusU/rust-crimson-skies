//! Evidence-report harness for task #548 (`docs/contracts/CLI-EVIDENCE.md`,
//! `schemas/evidence.schema.json`). Not named `accept_t548_*`: it is not part of
//! the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t548_ --include-ignored 2>&1 |
//!    tee private/evidence/T548/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T548 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t548_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_t548 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T548/acceptance.json
//!    --artifact-root private/evidence/T548 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T548.json`.
//!
//! `blast-rule.json` is a second production observation: every retail rocket
//! record's blast-relevant numbers as `read_original_rocket_blasts` returns
//! them, and the executable's SHA-256 with the byte windows the findings rest
//! on. Identifiers, numbers and digests only; no original display text.
//!
//! **On `unknowns`.** `--require-pass` rejects a nonempty `unknowns` list, and
//! this task has no unresolved issue inside its own assertions. The original
//! facts it leaves unmeasured are fidelity limitations, named with their
//! affected content in `LIMITATIONS` below, in `blast-rule.json` (a hashed
//! artifact) and in `docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`,
//! so none is removed from machine-readable evidence to turn a validator green.
//! The claim is `implemented`; `retail` is read access, not an original run.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_content::ordnance::read_original_rocket_blasts;

const RETAIL_TESTS: [&str; 3] = [
    "accept_t548_retail_rocket_records_declare_one_radius_and_one_channel_pair",
    "accept_t548_retail_radius_drives_the_measured_falloff",
    "accept_t548_image_pins_the_falloff_the_target_clear_and_the_timer_fallback",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR and CS_ENGINE_IMAGE set); this report is derived from the recorded log plus a second production pass recorded in blast-rule.json. MEASURED: the fifteen ROCKET records of ZBD/zrdr.zbd weapons.zrd through cs_content::ordnance::read_original_rocket_blasts (IMPACT_PROXIMITY, DETONATION_DISTANCE, DETONATION_TIME, ARMOR_DAMAGE, HEALTH_DAMAGE, LOCK_ON and the full key set, pinned at 41 names with none of EXPIRES, MINE, DETONATE_AT_RANGE, HIT_OWNER, REMOTE_DETONATE, TETHER_GUIDED or MULTI_TARGET); and byte windows of the owner-supplied decrypted executable (sha-256 recorded) carrying the area falloff (1 - d^2/r^2 with r = IMPACT_PROXIMITY, MINE flat), the target-pointer clear that defines a lost target, the target-pointer test that skips the DETONATION_DISTANCE proximity branch, and the DETONATION_TIME timer fallback. The rule: the original has no lost-target Detonate behavior; a lost target skips the proximity trigger so the rocket detonates on its timer or at the end of RANGE, and every detonation damages every object inside IMPACT_PROXIMITY with armor and health damage scaled by 1 - d^2/r^2. cs_sim now models that blast (AreaEffect::blast_fraction, OrdnanceRuntime::route_detonation_area), tested in CI with synthetic geometry. FIDELITY LIMITATIONS (unmeasured, none claimed): see the limitations array of blast-rule.json - the unit scale of IMPACT_PROXIMITY (claim t548.limit.units, resolving task: follow-up filed with Rally), the projectile +0x678 multiplier (t548.limit.scale), the +0xbc object field and 0xa1d7d4 gate in the apply loop (t548.limit.recipient_gate), whether the area entry value is a squared distance (t548.limit.entry_value), what removes a projectile past RANGE (t548.limit.range_end), the effect of the SONIC, FLASH, BEEPER, TANGLER and SMOKE_SCREEN keys (t548.limit.status_keys) and the RANGE_MINIMUM node call (t548.limit.range_minimum). route_detonation_area is not yet called by step_ordnance_session (t548.limit.session_wiring). The claim is implemented: this awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

const LIMITATIONS: [(&str, &str, &str, &str); 8] = [
    (
        "t548.limit.units",
        "the unit scale of IMPACT_PROXIMITY and DETONATION_DISTANCE",
        "any metre radius for the fifteen rocket records; no AreaEffect is declared from them",
        "follow-up task filed with Rally (unit calibration, see the F16-E finding)",
    ),
    (
        "t548.limit.scale",
        "the meaning and writer of the projectile +0x678 multiplier read in the blast radius and the damage product",
        "every area damage value",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.recipient_gate",
        "the +0xbc object field and the 0xa1d7d4 global that gate which objects the apply loop damages",
        "which objects a blast reaches",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.entry_value",
        "whether the area query's entry value is a squared distance (inferred from the stored squared radius; the write site was not read)",
        "the exact falloff at a given distance",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.range_end",
        "what removes a projectile that passes RANGE without detonating",
        "non-locked weapons without DETONATE_AT_RANGE",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.status_keys",
        "the effect of the SONIC, FLASH, BEEPER, BEEPER_SEEKER, TANGLER and SMOKE_SCREEN keys",
        "F28 non-negotiable 3 choke, stall and mark statuses",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.range_minimum",
        "what the RANGE_MINIMUM node call (0x4cd210, argument 1) does",
        "the torpedo",
        "follow-up task filed with Rally",
    ),
    (
        "t548.limit.session_wiring",
        "route_detonation_area is defined and tested in cs_sim but step_ordnance_session does not call it: the session holds no actor list to supply recipients",
        "a lost-target or timed blast in a live session still damages only the engagement's target",
        "follow-up task filed with Rally",
    ),
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t548_writes_the_acceptance_report() {
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
                    "{retail_test} did not run: #548 requires capability `retail`, run step 1 \
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
                 \"lock_on\": {}, \"keys\": {}}}",
                jstr(&rocket.record),
                number(rocket.impact_proximity),
                number(rocket.detonation_distance),
                number(rocket.detonation_time_s),
                number(rocket.armor_damage),
                number(rocket.health_damage),
                number(rocket.lock_on),
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
    let surface_path = evidence_dir.join("blast-rule.json");
    fs::write(
        &surface_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"weapons_member\": \"ZBD/zrdr.zbd weapons.zrd\", \"engine_image_sha256\": {}, \
             \"rockets\": [{}], \"limitations\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(&image_sha256),
            rocket_rows.join(", "),
            limitations.join(", "),
        ),
    )
    .expect("write blast-rule.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T548\",\n \"candidate_tree\": {},\n \
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
             (crates/cs_content/tests/evidence_report_t548.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T548` written relative to the
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
/// `accept_t548_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_t548_") {
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
                 \"blast-rule.json\"]}}",
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
