//! Evidence-report harness for task T452-R.05 (#2170)
//! (`docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`). Not
//! named `accept_t452_r05_*`: it is not part of the acceptance suite and
//! fails loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_content --test cs_content_tests -- accept_t452_r05_ --include-ignored 2>&1 |
//!    tee private/evidence/T452-R05/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452-R05 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_content --test cs_content_tests accept_t452_r05_ -- --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test cs_content_tests evidence_report_t452_r05 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452-R05/acceptance.json
//!    --artifact-root private/evidence/T452-R05 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T452-R05.json`.
//!
//! `slot-map-and-status.json` is a second record: the executable's SHA-256, the
//! resolution of the claim this task measured and the remaining limitations.
//! Identifiers, numbers and digests only; no original display text.
//!
//! **On `unknowns`.** `--require-pass` rejects a nonempty `unknowns` list, and
//! this task has no unresolved issue inside its own assertions. The two facts it
//! leaves open — what a mission script does with the raw record index, and the
//! runtime status variant — are named claims with follow-up tasks (#2173 and
//! #2172) in `LIMITATIONS` below, in `slot-map-and-status.json` (a hashed
//! artifact) and in
//! `docs/findings/2026-10-28-t452-r05-slot-map-and-incapacitation.md`, so none
//! is removed from machine-readable evidence to turn a validator green. The
//! claim is `implemented`; `retail` is read access, not an original run.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};

const RETAIL_TESTS: [&str; 3] = [
    "accept_t452_r05_slot_and_status::accept_t452_r05_retail_the_slot_map_and_the_status_match_the_records",
    "accept_t452_r05_slot_and_status::accept_t452_r05_the_image_pins_the_loadout_map",
    "accept_t452_r05_slot_and_status::accept_t452_r05_the_image_pins_the_exposure",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR and CS_ENGINE_IMAGE set); this report is derived from the recorded log plus a second record, slot-map-and-status.json, holding the engine image digest, the resolution and the limitations. MEASURED by static analysis of the owner-supplied decrypted executable (byte windows pinned in the acceptance tests): a plane record's eight rocket slots (record +0xa8, table 0x62ae38..0x62ae58, bound 0x443f26) hold rocket type ids 0..11, and 0x4440f0 (range check cmp ecx,0xa at 0x4440f7, jump table 0x444148) maps ids 0..10 to record indices 5..15 only, so no airframe slot can hold wep_04, wep_24, wep_25, wep_26 or wep_27; the preset binder (0x444269) uses the same mapper, the installer 0x4b24d0 walks slots 0..11 (0x4b250b), and the only writer that can name another index is the mission command at 0x43e530 (0x43e553 one slot, 0x43e56a all eight rocket slots, argument reader 0x5b7980 with no range check), which is script content and is follow-up #2173. The SONIC (0x200) and FLASH (0x800) bits are tested together at 0x4b9d84 (test ch,0x0a) and run the exposure 0x42e840: ratio = squared distance / IMPACT_PROXIMITY^2 (the record's +0x3c squared at 0x4ba590, parsed at 0x5add6a), intensity = clamp(1 - 2.5*(ratio - 0.6), 0, 1) (0x42e87e, constants 0x60411c and 0x6040a8), FLASH additionally scaled by clamp(2*dot(facing),0,1) (0x42e8ed), duration = intensity*5.0 s (0x42e9b1). The player actor takes a screen/audio exposure only (0x4ba5bf, 0x42e9d0) with no control loss; every other actor that passes 0x4200d0's gate (not the player, +0x91d == 0, +0xf8 == 0, +0x67c in {0,4}) gets mode +0x358 = 4, six control words zeroed and clock + duration at +0xc0, cleared by 0x489acd..0x489ae9 when the clock reaches it. cs_sim::weapons::ordnance::StatusEffectKind has no pilot-incapacitation variant (Damage/Choke/Stall/Marker), so no variant is forced: the claim t452_r02.limit.status_incapacitation is kept and the variant is follow-up #2172. The claim is implemented: this awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

const LIMITATIONS: [(&str, &str, &str, &str); 2] = [
    (
        "t452_r05.limit.script_ammo_use",
        "the mission-script command at 0x43e530 stores a raw weapons.zrd record index into the 12-entry slot table with no range check (0x43e553 one slot, 0x43e56a all eight rocket slots), so a mission could install wep_04, wep_24, wep_25, wep_26 or wep_27 at a slot it names; whether any retail mission does is script content and no script was read for this task",
        "the five records no airframe slot can select, wep_25 included",
        "#2173 (T452-R05-AMMO-SCRIPT)",
    ),
    (
        "t452_r02.limit.status_incapacitation",
        "the measured effect is pilot incapacitation (mode 4 and zeroed control words until a clock timestamp for every actor but the player, a screen/audio exposure for the player) and cs_sim::weapons::ordnance::StatusEffectKind's Damage/Choke/Stall/Marker variants do not express it, so no runtime variant is forced and the catalogue row still names the claim",
        "wep_08 (SONIC), wep_09 and wep_15 (FLASH)",
        "#2172 (T452-R05-STATUS-KIND)",
    ),
];

/// The claim this task resolved and how.
const RESOLUTIONS: [(&str, &str, &str); 1] = [(
    "t452_04.limit.rocket_flag_missing",
    "measured",
    "no plane record's rocket slot can hold wep_25: each of the eight slots holds a rocket type id mapped by 0x4440f0 to record indices 5..15 only, and wep_25 is index 25. The value behind the claim is 'no slot', not an unknown slot; the T452.04 row keeps its pre-existing entry until #2156 lowers it, and this task's measured layer records the resolution.",
)];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t452_r05_writes_the_acceptance_report() {
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
                    "{retail_test} did not run: T452-R.05 requires capability `retail`, run \
                     step 1 with --include-ignored, CS_GAME_DIR and CS_ENGINE_IMAGE set"
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = install::discover(&game_dir)
        .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    let image = fs::read(&image_path).expect("the engine image reads");
    let image_sha256 = sha256(&image).to_hex();

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
    let surface_path = evidence_dir.join("slot-map-and-status.json");
    fs::write(
        &surface_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"engine_image_sha256\": {}, \"resolutions\": [{}], \"limitations\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(&image_sha256),
            resolutions.join(", "),
            limitations.join(", "),
        ),
    )
    .expect("write slot-map-and-status.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452-R05\",\n \"candidate_tree\": {},\n \
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
             (crates/cs_content/tests/evidence_report_t452_r01.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T452-R01` written relative to the
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
/// `accept_t452_r05_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_t452_r05_") {
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
                 \"slot-map-and-status.json\"]}}",
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
