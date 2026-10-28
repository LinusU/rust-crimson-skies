//! Evidence-report harness for task T452-RUNTIME-REGISTER (#2171, the
//! integration step of #2156) — `docs/contracts/CLI-EVIDENCE.md`,
//! `schemas/evidence.schema.json`. Not named `accept_t452_register_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_ --include-ignored 2>&1 |
//!    tee private/evidence/T452-RUNTIME-REGISTER/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452-RUNTIME-REGISTER \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t452_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test cs_app_tests_a evidence_report_t452_register -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452-RUNTIME-REGISTER/acceptance.json
//!    --artifact-root private/evidence/T452-RUNTIME-REGISTER`
//!    (structurally: the report keeps every unexpressible measured value in
//!    `unknowns`, so `--require-pass` is not used — a limitation is never
//!    deleted to satisfy a validator, AGENTS.md "Evidence-bound tasks")
//! 4. Commit a copy as `docs/findings/evidence/T452-RUNTIME-REGISTER.json`.
//!
//! `original-registration.json` is a second production observation: the
//! measured catalogue lowered and registered — every declared id with its
//! outcome, the runtime family, its measured fields and every blocker with its
//! claim, content and resolving task. Identifiers, numbers and digests only: no
//! original display text and no script text.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::ordnance::{OriginalRegistrationOutcome, read_and_register_measured_ordnance};
use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_sim::weapons::{FuseRule, GuidanceRule, OrdnanceRegistry};

const RETAIL_TESTS: [&str; 3] = [
    "accept_t452_register_retail_the_measured_catalogue_registers_what_it_measures",
    "accept_t452_register_retail_a_registered_measured_component_launches",
    "accept_t452_inherit_retail_the_measured_wep_04_ramps_through_the_session",
];

/// Every id the measurement can express, in member order.
const EXPECTED_REGISTERED: [&str; 13] = [
    "wep_04", "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_14",
    "wep_15", "wep_24", "wep_25", "wep_27",
];

/// The ids a measured value keeps out of the runtime registry, in member order.
const EXPECTED_BLOCKED: [&str; 3] = ["wep_12", "wep_13", "wep_26"];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES=retail,gpu,audio); this report is derived from the recorded log plus a second production pass recorded in original-registration.json. MEASURED (identifiers, numbers, counts and digests only, no original display text, no script text): the sixteen hardpoint components T452.01's rules classify are read from the owner's weapons.zrd by read_original_component_table, read_original_weapon_records, read_original_weapon_media, read_original_ammo_declarations and read_original_weapons_provenance, then each is lowered by cs_app::ordnance::measured_ordnance_declaration and cs_app::ordnance::lower_ordnance and inserted into a real cs_sim::weapons::OrdnanceRegistry by cs_app::ordnance::register_measured_ordnance. Every value comes from a measurement: the family from the record's own keys (T452.01); the launch speed from VELOCITY, which T452-R.01 measured as metres per second along the launch direction, with Full inheritance for every record whose ACCELERATION is zero, and for a record whose ACCELERATION is non-zero the measured lateral-remainder ramp (T452-R.01): the speed scalar opens at |launcher| + 1e-4, its target is VELOCITY + |launcher|, every tick it gains dt*ACCELERATION clamped at the target, the launcher's velocity minus its launch-direction part is retained as the lateral remainder, and the recomposed velocity displaces the item by dt each step; the release delay measured as zero because T452-R.04 measured that the airframe row's refire interval delays the next release and never the current one; the per-slot stack from CLUSTER_SIZE (T452-R.03-B), cross-checked against the ammunition reader, one launch spending one unit (T452-R.02 section 5); the fuse from the record's own DETONATION_DISTANCE for a seeker, because T452-R.02 measured that the proximity path requires LOCK_ON, or from a declared DETONATION_TIME; the lost-target rule as coast-then-detonate at the end of RANGE, which T452-R.02 section 4 measured over LOCK_ON rather than over the ROCKET flag and T452-R.05 confirmed for the glidebomb wep_25; the lifetime as the record's own measured flight rule (RANGE over the flown speed, T452-R.04 section 2) evaluated at a stationary launcher and rounded up to the session's 30 Hz tick, because the runtime field is a fixed cap and the measured rule is range-bounded; the area as the record's IMPACT_PROXIMITY blast radius with the one-tick lifetime the runtime's bounded-area record needs for an instantaneous blast (T1318, T452-R.02 section 2); the damage channels from ARMOR_DAMAGE and HEALTH_DAMAGE; the marker status from BEEPER with its declared TIME (T452-R.02 section 1); and the media from the record's own FLYOUT MODEL and FIRE SOUND entry (T452-R.03-C). Two fields are designed vocabulary, not measured values, and say so: the mount key names the measured mount id (wep_05 -> weapon_mount_05) and the hardpoint kind is the project's own Underslung, because no record and no screen states where on the airframe a slot sits. The unit mass is the measured ABSENCE of a per-type term, recorded as zero: the airframe weight total adds a flat 480 weight per enabled rocket box and reads no weapon record field, so a loaded type contributes nothing to any weight total, and the flat 480/410 per box, charged per box rather than per round and in a weight unit whose conversion to kilograms is unmeasured, is kept as the named claim t452_register.limit.per_box_weight rather than converted into a kilogram figure. NOT CLAIMED: any original-run behavior (retail is read access and no agent ran the original executable); that the registered components fly as the original's do in any observable way. NOT YET EXPRESSIBLE, and therefore not registered, with the claim and the resolving task named per id: the measured silent expiry at the end of RANGE with no detonation (t452_register.limit.silent_expiry, task T452-REG-SILENT-EXPIRY, wep_12, wep_13, wep_26). A record with no declared FIRE SOUND (wep_26) is a measured silence the media record expresses as no sound, so it is not a blocker, and no retail hardpoint record lacks a FLYOUT MODEL (accept_t452_register_retail_every_hardpoint_record_declares_a_flyout_model). Nothing was defaulted to make an id register, and every measured value the runtime vocabulary cannot yet express for a declared id — the kept claims and the blockers — is listed in this report's `unknowns` with the record it sits on and the task that resolves it. The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated structurally with tools/validate_evidence.py - not with --require-pass, because the report's `unknowns` list carries every value this task could not express: the list is derived, not typed, from the registration report's own claims and blockers.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_register_writes_the_acceptance_report() {
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
        suite.passed >= 5 && suite.assertions.len() as u64 >= suite.passed,
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
                    "{retail_test} did not run: T452-RUNTIME-REGISTER requires capability \
                     `retail`, run step 1 with --include-ignored and CS_GAME_DIR set"
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
    let mut registry = OrdnanceRegistry::new();
    let report = read_and_register_measured_ordnance(&game_dir, &mut registry, 30)
        .expect("the installation's records read");
    assert_eq!(report.declared, 16, "sixteen declared ids");
    assert_eq!(report.registered_records(), EXPECTED_REGISTERED);
    assert_eq!(report.blocked_records(), EXPECTED_BLOCKED);

    let mut rows = Vec::new();
    for outcome in &report.outcomes {
        match outcome {
            OriginalRegistrationOutcome::Registered { record, family, .. } => {
                let id = cs_sim::weapons::OrdnanceId::try_new(
                    cs_types::content::ContentId::from_source(
                        cs_types::content::ContentKind::Weapon,
                        record,
                    )
                    .expect("a weapon id"),
                )
                .expect("the weapon namespace");
                let component = registry.get(&id).expect("a registered component");
                let projectile = component.as_projectile().expect("a projectile");
                let fuse = match projectile.fuse() {
                    FuseRule::Impact => "impact".to_owned(),
                    FuseRule::Proximity(fuse) => {
                        format!("proximity({})", fuse.trigger_radius_m())
                    }
                    FuseRule::Timed { ticks } => format!("timed({ticks})"),
                };
                let guidance = match projectile.guidance() {
                    GuidanceRule::Unguided => "unguided".to_owned(),
                    GuidanceRule::Targeted { lost_target } => {
                        format!("targeted({})", lost_target.label())
                    }
                    GuidanceRule::MarkerSeeker => "marker_seeker".to_owned(),
                };
                rows.push(format!(
                    "{{\"record\": {}, \"status\": \"registered\", \"family\": {}, \
                     \"mount\": {}, \"launch_speed_mps\": {}, \"inheritance\": {}, \
                     \"acceleration_mps2\": {}, \"release_delay_ticks\": {}, \
                     \"capacity_units\": {}, \"unit_mass_kg\": {}, \"arming\": {}, \
                     \"fuse\": {}, \"guidance\": {}, \"lifetime_ticks\": {}, \
                     \"area\": {}, \"armor_damage\": {}, \"internal_damage\": {}, \
                     \"status_effects\": {}, \"visual\": {}, \"sound\": {}}}",
                    jstr(record),
                    jstr(family.label()),
                    jstr(projectile.launch().mount().as_str()),
                    projectile.launch().launch_speed_mps(),
                    jstr(projectile.launch().inheritance().label()),
                    projectile
                        .launch()
                        .acceleration_mps2()
                        .map_or_else(|| "null".to_owned(), |rate| rate.to_string()),
                    projectile.launch().release_delay_ticks(),
                    projectile.stack().capacity_units(),
                    projectile.stack().unit_mass_kg(),
                    jstr(&projectile.arming().label()),
                    jstr(&fuse),
                    jstr(&guidance),
                    projectile.lifetime_ticks(),
                    projectile.area_effect().map_or_else(
                        || "null".to_owned(),
                        |area| format!(
                            "{{\"radius_m\": {}, \"lifetime_ticks\": {}}}",
                            area.radius_m(),
                            area.lifetime_ticks()
                        )
                    ),
                    projectile.channels().armor,
                    projectile.channels().internal,
                    projectile.status().len(),
                    jstr(projectile.media().visual().key()),
                    projectile
                        .media()
                        .sound()
                        .map_or_else(|| "null".to_owned(), |sound| jstr(sound.key())),
                ));
            }
            OriginalRegistrationOutcome::Blocked { record, blockers } => {
                rows.push(format!(
                    "{{\"record\": {}, \"status\": \"blocked\", \"blockers\": [{}]}}",
                    jstr(record),
                    blockers
                        .iter()
                        .map(|blocker| format!(
                            "{{\"field\": {}, \"claim\": {}, \"affects\": {}, \
                             \"resolving_task\": {}}}",
                            jstr(blocker.field),
                            jstr(&blocker.claim),
                            jstr(&blocker.affects),
                            jstr(blocker.resolving_task)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    let observation = format!(
        "{{\"task\": \"T452-RUNTIME-REGISTER\", \"declared\": {}, \"registered\": {}, \
         \"blocked\": {}, \"components\": [{}], \"claims_kept\": [{}]}}\n",
        report.declared,
        report.registered_records().len(),
        report.blocked_records().len(),
        rows.join(", "),
        report
            .claims_kept
            .iter()
            .map(|(record, claims)| format!(
                "{{\"record\": {}, \"claims\": [{}]}}",
                jstr(record),
                claims
                    .iter()
                    .map(|claim| jstr(claim))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let observation_path = evidence_dir.join("original-registration.json");
    fs::write(&observation_path, &observation).expect("write original-registration.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let unknowns = unknowns_json(&report);
    let report_json = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452-RUNTIME-REGISTER\",\n \
         \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \
         \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \
         \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \
         \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \
         \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \
         \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \
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
        unknowns,
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report_json).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Every value this stage could not express or could not carry, one
/// `"claim: content (resolving task)"` string per distinct claim, sorted.
///
/// Derived, never typed: the blockers and the kept claims come from the
/// production registration report, so a new limitation appears here the moment
/// the code records it, and none may be deleted to satisfy a validator.
fn unknowns_json(report: &cs_app::ordnance::OriginalRegistrationReport) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    for blocker in report.blockers() {
        let text = format!(
            "{}: {} ({}) — resolving task {}",
            blocker.claim, blocker.field, blocker.affects, blocker.resolving_task
        );
        if !rows.iter().any(|(claim, _)| *claim == blocker.claim) {
            rows.push((blocker.claim.clone(), text));
        }
    }
    for (record, claims) in &report.claims_kept {
        for claim in claims {
            if rows.iter().any(|(seen, _)| seen == claim) {
                continue;
            }
            let text = match cs_app::ordnance::original_kept_claim_context(claim) {
                Some((affects, task)) => {
                    format!("{claim}: {affects} (kept on {record}) — resolving task {task}")
                }
                None => {
                    format!("{claim}: kept on {record}; no measurement-backed consumer names it")
                }
            };
            rows.push((claim.clone(), text));
        }
    }
    rows.sort();
    format!(
        "[{}]",
        rows.iter()
            .map(|(_, text)| jstr(text))
            .collect::<Vec<_>>()
            .join(", ")
    )
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
                 \"original-registration.json\"]}}",
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
