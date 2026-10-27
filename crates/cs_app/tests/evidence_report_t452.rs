//! Evidence-report harness for task T452
//! (`docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`). Not
//! named `accept_t452_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_ --include-ignored 2>&1 |
//!    tee private/evidence/T452/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test cs_app_tests_a evidence_report_t452 --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452/acceptance.json
//!    --artifact-root private/evidence/T452 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T452.json`.
//!
//! `original-catalogue.json` is a second production observation: the measured
//! catalogue — every declared component id, its family, mount, guidance,
//! fuse (shape, sampling, radius), arming, the claims recorded against it, the
//! nitro record and registry coverage — as
//! [`cs_app::ordnance::read_original_catalogue`] returns it, catalogued into a
//! real `OrdnanceRegistry`. Identifiers, numbers and digests only — no original
//! display text and no script text.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::ordnance::{
    catalogue_measured_ordnance, original_catalogue_coverage, read_original_catalogue,
};
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_sim::weapons::{CataloguedFuse, CataloguedMount, OrdnanceRegistry};

const RETAIL_TESTS: [&str; 5] = [
    "accept_t452_retail_every_declared_component_id_resolves_in_the_registry",
    "accept_t452_retail_each_fuse_is_the_records_own_trigger",
    "accept_t452_retail_the_nitro_record_is_the_measured_one",
    "accept_t452_retail_every_entry_names_the_values_it_does_not_measure",
    "accept_t452_retail_the_projectile_component_type_is_still_the_runtime_one",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES=retail,gpu,audio); this report is derived from the recorded log plus a second production pass recorded in original-catalogue.json. MEASURED (identifiers, numbers, counts and digests only, no original display text, no script text): every weapon record T452.01's stated rules classify - sixteen in all: twelve guided rockets, two area-denial components, one aerial torpedo and one proximity flak - read through cs_content::ordnance::read_original_component_table and read_original_rocket_blasts, catalogued by cs_app::ordnance::read_original_catalogue and inserted with cs_app::ordnance::catalogue_measured_ordnance into a real cs_sim::weapons::OrdnanceRegistry, where every one of the sixteen ids resolves with a family, a mount, a guidance rule, a fuse and an arming rule; the thirty-two records no family rule reaches (the thirty-one CANNON machine and turret guns and the cannonball wep_28) are listed by name and never folded into a family. Per component the catalogue keeps only what the files state: the family from the record's keys; the mount as one of the eight rocket slots (the glidebomb wep_25 declares LOCK_ON and no ROCKET flag, so its mount is a named unknown, not an assumed slot); guidance Unguided or Targeted, with the measured lost-target rule coast-then-detonate-at-end-of-RANGE for every guided record task #454 measured, and a named unknown for wep_25 which that measurement does not cover; the fuse as the record's own DETONATION_DISTANCE sphere (shape sphere, boundary inclusive) sampled the way task T452.02 statically measured the engine to sample it - once per step at the item's start-of-step position, not the swept relative-path model - or the record's stated DETONATION_TIME duration in seconds (wep_15, 2 s), or a named unknown where the record states no trigger at all (the smoke record wep_13 and the flak record wep_26, whose only stated radius is its IMPACT_PROXIMITY blast radius, not a trigger); arming on release, the measured rule, because T452.02 found no arming condition on the proximity path other than the fuse's own live conditions and RANGE_MINIMUM gates impact detonations only. The nitro booster is not a wep_ record - no record, no id - so it is recorded as actor state: the header's MPOUT_CHK_NITRO control, capacity 30.0, consumption 4.0 per engine delta, recovery 1.0 per delta on every step, a start at 99% of capacity and a cut below 5%, and the measured 1.8 engine-value replacement and 0.8 result factor. NOT CLAIMED: any original-run behavior (retail is read access and no agent ran the original executable); any statement that the original's rockets fly at the recorded VELOCITY, live for any duration, apply any status effect or mount in the recorded slot. What each catalogued component still does not measure is named per entry by claim id and field rather than defaulted: t452_04.limit.velocity_unit (the VELOCITY unit, fifteen records), t452_04.limit.release_delay (what FIRE_RATE delays and in which direction, sixteen records), t452_04.limit.no_expires_key (the member carries no EXPIRES key at all, fifteen records), t452_04.limit.area_lifetime (fifteen records), t452_04.limit.area_radius (the two area-denial records), t452_04.limit.status_effect_kind (seven records) and t452_04.limit.status_effect_duration (four records), t452_04.limit.rocket_flag_missing (wep_25's slot), t454.limit.swarm (the eleven clustered records), t452.limit.fuse (wep_13 and wep_26), t452.limit.lost_target (wep_25), t452.limit.nitro_activation_rule (the measured charge-bounded rule has no NitroActivationRule variant, so it is recorded and not lowered to a rule it is not; resolving task #452 follow-up), t452.limit.nitro_tradeoff (the measured 1.8 replaces an engine-record value, so no newtons and no authority multiplier are asserted), and t452.limit.runtime_lowering (every entry: the record states no stack capacity or unit mass, no media keys and no equipment rules, so a runtime component would fabricate all three - the entries are catalogued, and no original id is registered as flyable). The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_writes_the_acceptance_report() {
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
                    "{retest} did not run: T452 requires capability `retail`, run step 1 with \
                     --include-ignored and CS_GAME_DIR set",
                    retest = retail_test
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = install::discover(&game_dir)
        .unwrap_or_else(|error| panic!("the installation must be discoverable: {error:?}"))
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    // Second observation: the same production path, run again.
    let catalogue = read_original_catalogue(&game_dir).expect("the retail catalogue reads");
    let mut registry = OrdnanceRegistry::new();
    catalogue_measured_ordnance(&catalogue, &mut registry).expect("no duplicate ids");
    let coverage = original_catalogue_coverage(&catalogue, &registry);
    assert!(
        coverage.is_complete() && coverage.declared == 16,
        "the measured catalogue must resolve completely: {coverage:?}"
    );

    let mut rows = Vec::new();
    for declared in &catalogue.entries {
        let entry = &declared.entry;
        let mount = match entry.mount() {
            CataloguedMount::RocketSlot { slots } => {
                format!("{{\"kind\": \"rocket_slot\", \"slots\": {slots}}}")
            }
            CataloguedMount::Unknown { claim } => {
                format!("{{\"kind\": \"unknown\", \"claim\": {}}}", jstr(claim))
            }
        };
        let (fuse, fuse_claim, shape, sampling, radius) = match entry.fuse() {
            CataloguedFuse::Impact => (
                jstr("impact"),
                "null".to_owned(),
                "null".to_owned(),
                "null".to_owned(),
                "null".to_owned(),
            ),
            CataloguedFuse::Proximity { fuse, shape } => (
                jstr("proximity"),
                "null".to_owned(),
                jstr(shape.label()),
                jstr(fuse.sampling().label()),
                format!("{}", fuse.trigger_radius_m()),
            ),
            CataloguedFuse::TimedSeconds { seconds } => (
                jstr("timed_s"),
                "null".to_owned(),
                "null".to_owned(),
                "null".to_owned(),
                format!("{seconds}"),
            ),
            CataloguedFuse::Undeclared { claim } => (
                jstr("undeclared"),
                jstr(claim),
                "null".to_owned(),
                "null".to_owned(),
                "null".to_owned(),
            ),
        };
        rows.push(format!(
            "{{\"record\": {}, \"family\": {}, \"mount\": {mount}, \"guidance\": {}, \
             \"lost_target\": {}, \"fuse\": {fuse}, \"fuse_claim\": {fuse_claim}, \
             \"shape\": {shape}, \"sampling\": {sampling}, \
             \"radius_or_seconds\": {radius}, \"arming\": {}, \"unmeasured\": [{}]}}",
            jstr(&declared.record),
            jstr(entry.family().label()),
            jstr(guidance_label(entry)),
            entry
                .lost_target()
                .map_or_else(|| "null".to_owned(), |behavior| jstr(behavior.label())),
            jstr(entry.arming().label()),
            entry
                .unmeasured()
                .iter()
                .map(|unknown| format!(
                    "{{\"field\": {}, \"claim\": {}, \"reason_len\": {}}}",
                    jstr(&unknown.field),
                    jstr(&unknown.claim),
                    unknown.reason.len()
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let nitro = &catalogue.nitro;
    let observation = format!(
        "{{\"task\": \"T452\", \"coverage\": {{\"declared\": {}, \"catalogued\": {}, \
         \"complete\": {}, \"families\": [{}]}}, \"layout\": {{\"rocket_slots\": {}, \
         \"ammunition_slots\": {}, \"hardpoint_points\": {}}}, \"components\": [{}], \
         \"excluded\": [{}], \"nitro\": {{\"control\": {}, \"control_id\": {}, \
         \"component_records\": {}, \"capacity\": {}, \"consumption_per_dt\": {}, \
         \"recovery_per_dt\": {}, \"start_min_fraction\": {}, \"stop_below_fraction\": {}, \
         \"running_engine_value\": {}, \"running_result_factor\": {}, \
         \"activation_is_while_held\": {}, \"extra_thrust_n\": null, \"claims\": [{}]}}, \
         \"claims\": [{}], \"table_unknowns\": [{}]}}\n",
        coverage.declared,
        coverage.catalogued,
        coverage.is_complete(),
        coverage
            .families
            .iter()
            .map(|(family, count)| format!(
                "{{\"family\": {}, \"count\": {count}}}",
                jstr(family.label())
            ))
            .collect::<Vec<_>>()
            .join(", "),
        catalogue.rocket_slots,
        catalogue.ammunition_slots,
        catalogue.hardpoint_points,
        rows.join(", "),
        catalogue
            .excluded
            .iter()
            .map(|name| jstr(name))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(nitro.control.0),
        nitro.control.1,
        nitro.component_records,
        nitro.capacity,
        nitro.consumption_per_dt,
        nitro.recovery_per_dt,
        nitro.start_min_fraction,
        nitro.stop_below_fraction,
        nitro.running_engine_value,
        nitro.running_result_factor,
        nitro.activation_is_while_held(),
        nitro
            .unmeasured
            .iter()
            .map(|unknown| jstr(&unknown.claim))
            .collect::<Vec<_>>()
            .join(", "),
        coverage
            .claims
            .iter()
            .map(|claim| jstr(claim))
            .collect::<Vec<_>>()
            .join(", "),
        catalogue
            .unmeasured
            .iter()
            .map(|unknown| jstr(&unknown.claim))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let observation_path = evidence_dir.join("original-catalogue.json");
    fs::write(&observation_path, &observation).expect("write original-catalogue.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \
         \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \
         \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(),
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
             (crates/cs_app/tests/evidence_report_t452.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T452` written relative to the
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

fn engine_json() -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d"))
    )
}

fn guidance_label(entry: &cs_sim::weapons::CataloguedOrdnance) -> &'static str {
    if entry.guidance().is_targeted() {
        "targeted"
    } else {
        "unguided"
    }
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
/// `accept_t452_` tests from a recorded `cargo test` output.
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
            // The suite is a module of `tests/suite_a.rs`: drop the module path.
            let name = after[..separator]
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with("accept_t452_") {
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

// --------------------------------------------------------------- encoding ---

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {}, \"evidence\": [\"cargo-test.log\", \
                 \"original-catalogue.json\"]}}",
                jstr(name),
                jstr(status)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(path, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {}, \"kind\": {}}}",
                jstr(path),
                jstr(digest),
                jstr(kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn str_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| jstr(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A JSON string literal.
fn jstr(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(i64::try_from(days).expect("days fit"));
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`, for a UTC date without a dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        u32::try_from(m).expect("month fits"),
        u32::try_from(d).expect("day fits"),
    )
}
