//! Evidence-report harness for task T452.04
//! (`docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`). Not
//! named `accept_t452_04_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t452_04_ --include-ignored 2>&1 |
//!    tee private/evidence/T452.04/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T452.04 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t452_04_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test cs_content_tests evidence_report_t452_04 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T452.04/acceptance.json
//!    --artifact-root private/evidence/T452.04 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T452.04.json`.
//!
//! `component-table.json` is a second production observation: the mount layout
//! the two screens declare, the per-component rows, the per-claim row counts and
//! the table-level unknown, as
//! [`read_original_component_table`] returns them. Identifiers, numbers and
//! digests only — no original content.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{self, content_fingerprint, fingerprint, sha256};
use cs_content::ordnance::{
    OriginalClusterShape, OriginalLifetime, OriginalMountDeclaration, read_original_component_table,
};

const RETAIL_TESTS: [&str; 5] = [
    "accept_t452_04_retail_the_two_screens_declare_the_layout",
    "accept_t452_04_retail_every_component_row_is_read_from_its_record",
    "accept_t452_04_retail_no_record_declares_a_lifetime_key",
    "accept_t452_04_retail_a_guns_cluster_size_is_its_ammunition_count",
    "accept_t452_04_retail_every_unknown_names_the_rows_it_affects",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set); this report is derived from the recorded log plus a second production pass recorded in component-table.json. MEASURED (identifiers, numbers, counts and digests only, no original display text, no script text): the mount layout the installation's two ordnance screens declare, read through cs_assets' production ROF mount and cs_formats' bounded member decoder and parsed from the screens' own `object <array>[n]` declarations and their fill loops - eight rocket slots (object RKA[8], for (BA = 0; BA < 8; BA++)), four gun/ammunition slots (object QKA[4], for (BA = 0; BA < 4; BA++)) and two hardpoint points (object DT[2], for (R = 0; R < 2; R++)), each bound stated twice and agreeing, and each ordnance array deactivating its own empty slot (RKA and QKA each call deactivate on it; HARDPOINTS.SCRIPT has no deactivate and always shows both points). Per component, cs_content::ordnance::read_original_component_table reads every weapon record T452.01's stated rules classify (sixteen: twelve guided, two area-denial, one torpedo, one proximity flak) and records what the record itself declares: VELOCITY, FIRE_RATE, CLUSTER_SIZE, IMPACT_PROXIMITY, ARMOR_DAMAGE/HEALTH_DAMAGE/DAMAGE, DETONATION_TIME, and each status/effect flag with the duration the key carries (TANGLER 2 s, SMOKE_SCREEN 8 s, BEEPER 20 s, all with the TIME unit word); the thirty-two records no family rule reaches (the thirty-one CANNON machine and turret guns, and the cannonball wep_28) are listed by name and excluded rather than lowered. NOT CLAIMED: any original-run behavior (retail is read access, no agent ran the original); the per-airframe layout (which component fills which slot on a given plane) is in the plane records in the engine image and is recorded as the named unknown t452_04.limit.per_airframe_layout; the VELOCITY unit (t452_04.limit.velocity_unit, fifteen records), what FIRE_RATE delays and in which direction (t452_04.limit.release_delay, sixteen records), the projectile lifetime (the member carries no EXPIRES key at all: t452_04.limit.no_expires_key, fifteen records), the area-denial effect's radius for the two records that state no area (t452_04.limit.area_radius, two records) and the area's lifetime (t452_04.limit.area_lifetime, fifteen records), the status effect's gameplay kind (t452_04.limit.status_effect_kind, seven records), a status effect's missing duration (t452_04.limit.status_effect_duration, four records), which slot holds a record with no ROCKET flag (t452_04.limit.rocket_flag_missing, wep_25), and what each clustered sub-munition does (the carried t454.limit.swarm, eleven records). Every value the member does state is recorded exactly as stored, including the f32 rounding the writer left in FIRE_RATE (wep_10's 0.3 is 0.30000001192092896). The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t452_04_writes_the_acceptance_report() {
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
                    "{retail_test} did not run: T452.04 requires capability `retail`, run step \
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

    // Second observation: the same production reader, run again.
    let table = read_original_component_table(&game_dir).expect("the retail table reads");
    assert_eq!(table.rows.len(), 16, "sixteen classified components");

    let array = |slot: &cs_content::ordnance::OriginalSlotArray| {
        format!(
            "{{\"member\": {}, \"array\": {}, \"slots\": {}, \"loop_bound\": {}, \
             \"deactivates_empty_slots\": {}}}",
            jstr(&slot.member),
            jstr(&slot.array),
            slot.slots,
            slot.loop_bound,
            slot.deactivates_empty_slots
        )
    };
    let layout = format!(
        "{{\"rocket_slots\": {}, \"ammunition_slots\": {}, \"hardpoint_points\": {}, \
         \"consistent\": {}, \"findings\": {}}}",
        array(&table.layout.rocket_slots),
        array(&table.layout.ammunition_slots),
        array(&table.layout.hardpoint_points),
        table.layout.is_consistent(),
        table.layout.findings.len()
    );
    let rows: Vec<String> = table
        .rows
        .iter()
        .map(|row| {
            let mount = match &row.mount {
                OriginalMountDeclaration::RocketSlot { slots } => {
                    format!("{{\"rocket_slot\": {slots}}}")
                }
                OriginalMountDeclaration::Unknown { claim, .. } => {
                    format!("{{\"unknown\": {}}}", jstr(claim))
                }
            };
            let lifetime = match row.lifetime {
                OriginalLifetime::Declared { seconds, key } => format!(
                    "{{\"declared\": {{\"seconds\": {seconds}, \"key\": {}}}}}",
                    jstr(key)
                ),
                OriginalLifetime::Undeclared => "{\"undeclared\": true}".to_owned(),
            };
            let cluster = match row.cluster {
                OriginalClusterShape::Volley { munitions } => {
                    format!("{{\"volley\": {munitions}}}")
                }
                OriginalClusterShape::AmmunitionCount { rounds } => {
                    format!("{{\"ammunition_count\": {rounds}}}")
                }
                OriginalClusterShape::Undeclared => "{\"undeclared\": true}".to_owned(),
            };
            let statuses: Vec<String> = row
                .statuses
                .iter()
                .map(|status| {
                    format!(
                        "{{\"key\": {}, \"duration_s\": {}, \"duration_unit\": {}}}",
                        jstr(status.key),
                        opt_num(status.duration_s),
                        opt_str(status.duration_unit.as_deref())
                    )
                })
                .collect();
            let unknowns: Vec<String> = row
                .unknowns
                .iter()
                .map(|unknown| jstr(&unknown.claim))
                .collect();
            format!(
                "{{\"record\": {}, \"family\": {}, \"mount\": {mount}, \"velocity\": {}, \
                 \"fire_rate\": {}, \"lifetime\": {lifetime}, \"area_radius_m\": {}, \
                 \"area_lifetime_s\": {}, \"damage_armor\": {}, \"damage_health\": {}, \
                 \"damage_single\": {}, \"statuses\": [{}], \"cluster\": {cluster}, \
                 \"unknowns\": [{}]}}",
                jstr(&row.record),
                jstr(row.family.label()),
                opt_num(row.velocity),
                opt_num(row.fire_rate),
                opt_num(row.area.radius_m),
                opt_num(row.area.lifetime_s),
                opt_num(row.damage.armor),
                opt_num(row.damage.health),
                opt_num(row.damage.single),
                statuses.join(", "),
                unknowns.join(", ")
            )
        })
        .collect();
    let claims: Vec<String> = table
        .claims()
        .into_iter()
        .map(|claim| {
            let affected: Vec<String> = table
                .rows
                .iter()
                .filter(|row| row.names_claim(&claim))
                .map(|row| jstr(&row.record))
                .collect();
            format!(
                "{{\"claim\": {}, \"affected\": [{}]}}",
                jstr(&claim),
                affected.join(", ")
            )
        })
        .collect();
    let excluded: Vec<String> = table.excluded.iter().map(|record| jstr(record)).collect();
    let table_unknowns: Vec<String> = table
        .unknowns
        .iter()
        .map(|unknown| {
            format!(
                "{{\"claim\": {}, \"reason\": {}}}",
                jstr(&unknown.claim),
                jstr(&unknown.reason)
            )
        })
        .collect();
    let table_path = evidence_dir.join("component-table.json");
    fs::write(
        &table_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \
             \"layout\": {layout}, \"rows\": [{}], \"claims\": [{}], \"excluded\": [{}], \
             \"table_unknowns\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            rows.join(", "),
            claims.join(", "),
            excluded.join(", "),
            table_unknowns.join(", "),
        ),
    )
    .expect("write component-table.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&table_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T452.04\",\n \"candidate_tree\": {},\n \
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
             (crates/cs_content/tests/evidence_report_t452_04.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T452.04` written relative to the
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
/// `accept_t452_04_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_t452_04_") {
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
                 \"component-table.json\"]}}",
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
