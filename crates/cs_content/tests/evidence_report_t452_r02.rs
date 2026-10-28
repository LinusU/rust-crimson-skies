//! Evidence-report harness for task T452-R02
//! (`docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`). Not
//! named `accept_t452_r02_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test -p cs_content --locked --test cs_content_tests -- accept_t452_r02_ --include-ignored 2>&1 |
//!    tee private/evidence/T452-R02/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452-R02 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test -p cs_content --locked --test cs_content_tests -- accept_t452_r02_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test cs_content_tests evidence_report_t452_r02 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452-R02/acceptance.json
//!    --artifact-root private/evidence/T452-R02 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T452-R02.json`.
//!
//! `behavior.json` is a second production observation: every classified row's
//! measured effect action and duration, area radius/lifetime and their sources,
//! end of flight, lost-target rule, clustered launch, kept claims and resolved
//! claims, as [`measure_component_behavior`] returns them over
//! [`read_original_component_table`]. Identifiers, numbers and digests only —
//! no original content.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_content::ordnance::{
    MeasuredAreaSource, MeasuredEffectAction, MeasuredEffectDuration, MeasuredRangeEnd,
    OriginalBehaviorMeasurement, measure_component_behavior, read_original_component_table,
};

const RETAIL_TESTS: [&str; 2] = [
    "accept_t452_r02_the_image_pins_every_measured_site",
    "accept_t452_r02_retail_the_measured_facts_read_the_original_rows",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR and CS_ENGINE_IMAGE set); this report is derived from the recorded log plus a second production pass recorded in behavior.json. MEASURED (identifiers, numbers and digests only, no original display text, no script text, no disassembly committed): static analysis of the owner-supplied decrypted engine image (sha-256 43540fc97347210d..., image base 0x400000, 24 byte windows re-read by accept_t452_r02_the_image_pins_every_measured_site) for what each effect flag does: BEEPER places a timed marker on the struck actor whose duration is the record TIME (default 5.0 s, wep_10 20 s); SONIC and FLASH run the exposure routine 0x42e840, whose intensity is clamp(1 - 2.5 * (ratio - 0.6), 0, 1) with ratio the per-hit measure over the attacker field +0x3c squared, FLASH additionally scaled by the facing dot product, and whose incapacitation duration is intensity * 5.0 s; TANGLER builds an area field (0x4b94e0) of radius RADIUS (default 10.0 m, squared at +0x14) lasting TIME (default 5.0 s, wep_12 2 s) that re-applies the weapon per-hit routine to every actor inside the radius each tick; SMOKE_SCREEN creates a scene emitter node (0x4b8d50) lasting TIME (default 5.0 s, wep_13 8 s) with no gameplay radius and, for a record that states no TIME, the image's own 5.0 s default with an ImageDefault provenance label; BEEPER_SEEKER installs the per-step retarget callback 0x441780. The end of flight is gated at 0x5b01f9..0x5b0211 on LOCK_ON (0x8000, set by the LOCK_ON key) without EXPIRES (0x80), or on DETONATE_AT_RANGE (0x2000000); a record with neither is marked dead at 0x5b0318 without detonating, which is wep_12, wep_13 and wep_26 -- wep_12's DETONATION_DISTANCE cannot reach the LOCK_ON-gated proximity branch. The steering (0x5af7f4) and proximity (0x5b0216) guards test LOCK_ON, not ROCKET, so wep_25 inherits the twelve-record lost-target rule. CLUSTER_SIZE (+0x0c) is a per-launch countdown: one launch pass calls the one-projectile builder 0x5aef40 exactly once (0x4b7289), the count is decremented at 0x4b72be and the loop stops at 0, so one projectile per sub-munition. Per component the same production reader (cs_content::ordnance::read_original_component_table) is measured by measure_component_behavior: wep_12 gets radius 10 m, lifetime 2 s and the silent end of flight; wep_13 lifetime 8 s and no measured radius; wep_08/wep_09 the exposure formula with no forced runtime kind; wep_10 the marker kind; wep_11 the beeper-seeker claim; wep_12, wep_13 and wep_26 expire silently; wep_25 detonates and resolves the lost-target claim; every volley launches one projectile per sub-munition. NOT CLAIMED: any original-run behavior (retail is read access and no agent ran the original executable); what a zero-damage tangler does to a flight model (t452_r02.limit.tangler_flight_effect), a gameplay radius for smoke (t452_r02.limit.smoke_radius), a StatusEffectKind for pilot incapacitation (t452_r02.limit.status_incapacitation) and a GuidanceRule for repeated beeper retargeting (t452_r02.limit.beeper_seeker_guidance). The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_r02_writes_the_acceptance_report() {
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
                    "{retail_test} did not run: T452-R02 requires capability `retail`, run step \
                     1 with --include-ignored and CS_GAME_DIR set"
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = install::discover(&game_dir)
        .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    // Second observation: the same production reader, measured again.
    let table = read_original_component_table(&game_dir).expect("the retail table reads");
    assert_eq!(table.rows.len(), 16, "sixteen classified components");
    let measured: Vec<(&str, OriginalBehaviorMeasurement)> = table
        .rows
        .iter()
        .map(|row| (row.record.as_str(), measure_component_behavior(row)))
        .collect();
    let (_, tangler) = measured
        .iter()
        .find(|(name, _)| *name == "wep_12")
        .expect("wep_12 is measured");
    let tangler_area = tangler.area.expect("wep_12 has an area");
    assert_eq!(tangler_area.radius_m, Some(10.0));
    let (_, smoke) = measured
        .iter()
        .find(|(name, _)| *name == "wep_13")
        .expect("wep_13 is measured");
    let smoke_area = smoke.area.expect("wep_13 has an area");
    assert!(matches!(
        smoke_area.radius_source,
        MeasuredAreaSource::Unmeasured { .. }
    ));
    let (_, glidebomb) = measured
        .iter()
        .find(|(name, _)| *name == "wep_25")
        .expect("wep_25 is measured");
    assert_eq!(
        glidebomb.fuse.range_end,
        MeasuredRangeEnd::Detonates,
        "wep_25 is a LOCK_ON record"
    );
    let (_, flak) = measured
        .iter()
        .find(|(name, _)| *name == "wep_26")
        .expect("wep_26 is measured");
    assert_eq!(flak.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);

    let effects: Vec<String> = measured
        .iter()
        .flat_map(|(record, behavior)| {
            behavior.effects.iter().map(move |effect| {
                let action = match effect.action {
                    MeasuredEffectAction::TimedMarker => "timed_marker",
                    MeasuredEffectAction::AreaField => "area_field",
                    MeasuredEffectAction::ExposureIncapacitation => "exposure_incapacitation",
                    MeasuredEffectAction::GuidanceModifier => "guidance_modifier",
                };
                let duration = match effect.duration {
                    MeasuredEffectDuration::RecordTime => "record_time".to_owned(),
                    MeasuredEffectDuration::ImageDefaultSeconds(seconds) => {
                        format!("default_{seconds}s")
                    }
                    MeasuredEffectDuration::Exposure {
                        max_seconds,
                        full_intensity_ratio,
                        zero_intensity_ratio,
                        facing_scaled,
                    } => format!(
                        "exposure_max_{max_seconds}s_full_{full_intensity_ratio}_zero_{zero_intensity_ratio}_facing_{facing_scaled}"
                    ),
                    MeasuredEffectDuration::None => "none".to_owned(),
                };
                format!(
                    "{{\"record\": {}, \"key\": {}, \"action\": {action}, \"duration\": {}, \"kind\": {}, \"pin\": {}}}",
                    jstr(record),
                    jstr(effect.key),
                    jstr(&duration),
                    opt_str(effect.runtime_kind.map(|kind| kind.label())),
                    jstr(&format!("0x{:x}+{}", effect.pin.va, effect.pin.len))
                )
            })
        })
        .collect();
    let rows: Vec<String> = measured
        .iter()
        .map(|(record, behavior)| {
            let area = behavior.area.map_or_else(
                || "null".to_owned(),
                |area| {
                    let source = |source: MeasuredAreaSource| match source {
                        MeasuredAreaSource::RecordKey { key } => format!("record:{}", jstr(key)),
                        MeasuredAreaSource::ImageDefault { value } => {
                            format!("image_default:{value}")
                        }
                        MeasuredAreaSource::InstantaneousBlast => "instantaneous_blast".to_owned(),
                        MeasuredAreaSource::Unmeasured { claim } => {
                            format!("unmeasured:{}", jstr(claim))
                        }
                    };
                    format!(
                        "{{\"radius_m\": {}, \"radius_source\": {}, \"lifetime_s\": {}, \"lifetime_source\": {}}}",
                        opt_num(area.radius_m),
                        jstr(&source(area.radius_source)),
                        opt_num(area.lifetime_s),
                        jstr(&source(area.lifetime_source))
                    )
                },
            );
            let range_end = match behavior.fuse.range_end {
                MeasuredRangeEnd::Detonates => "detonates",
                MeasuredRangeEnd::ExpiresSilently => "expires_silently",
            };
            let lost_target = behavior.lost_target.map_or_else(
                || "null".to_owned(),
                |rule| {
                    format!(
                        "{{\"behavior\": {}, \"requires_lock_on\": {}}}",
                        jstr(rule.behavior),
                        rule.requires_lock_on
                    )
                },
            );
            let swarm = behavior.swarm.map_or_else(
                || "null".to_owned(),
                |swarm| {
                    format!(
                        "{{\"munitions\": {}, \"projectiles_per_launch\": {}}}",
                        swarm.munitions, swarm.projectiles_per_launch
                    )
                },
            );
            let claims: Vec<String> = behavior
                .claims
                .iter()
                .map(|claim| jstr(&claim.claim))
                .collect();
            let resolved: Vec<String> = behavior.resolved.iter().map(|claim| jstr(claim)).collect();
            format!(
                "{{\"record\": {}, \"area\": {area}, \"range_end\": {range_end}, \"lost_target\": {lost_target}, \"swarm\": {swarm}, \"claims\": [{}], \"resolved\": [{}]}}",
                jstr(record),
                claims.join(", "),
                resolved.join(", ")
            )
        })
        .collect();
    let behavior_path = evidence_dir.join("behavior.json");
    fs::write(
        &behavior_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \"rows\": [{}], \"effects\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            rows.join(", "),
            effects.join(", "),
        ),
    )
    .expect("write behavior.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&behavior_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452-R02\",\n \"candidate_tree\": {},\n \
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
             (crates/cs_content/tests/evidence_report_t452_r02.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T452-R02` written relative to the
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
/// `accept_t452_r02_` tests from a recorded `cargo test` output.
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
            // The suite is a module of `tests/main.rs`: drop the module path.
            let name = after[..separator]
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_t452_r02_") {
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
                 \"behavior.json\"]}}",
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

fn opt_str(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), jstr)
}

fn opt_num(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_owned(), |number| number.to_string())
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
