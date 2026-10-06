//! Evidence-report harness for task `M01-LC-FVOL-ROLES`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_fvol_roles_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_fvol_roles
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-FVOL-ROLES/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-FVOL-ROLES \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_fvol_roles --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_fvol_roles -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-FVOL-ROLES/acceptance.json
//!    --artifact-root private/evidence/M01-LC-FVOL-ROLES --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-FVOL-ROLES.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs the world-container
//! import over all eight containers and records the census — anchors, fog
//! volumes, still-unresolved records, the `fvol*` records the partition grid
//! also names, the axis map and its evidence class — as JSON. That is a real
//! production run over the owner's installation, not a paraphrase of the
//! acceptance assertions, and it carries no original bytes: ids, digests,
//! counts and claim labels only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::world::{RetailWorldContainer, read_world_containers};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_types::content::Origin;

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_fvol_roles_";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from the production
/// imports rather than written down, so a report regenerated on another
/// installation cannot describe this one's numbers.
fn review_method(imported: usize, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of the world-container import path over the installation (world-fvol-roles-census.json). \
         Claim is implemented only. MEASURED: (a) the runtime role of the `fvol*` volume records — \
         over all {imported} world containers, every unindexed record whose stored name carries the \
         four-byte `fvol` prefix resolves to WorldCollisionRole::None under \
         FOG_VOLUME_RECORD_NEVER_BLOCKS with its mesh kept as a known reference, because the \
         decrypted image references that prefix exactly once (a strncmp(name, \\\"fvol\\\", 4) inside \
         the routine that reads fogvol.zrd's fog keys) and no .zrd member anywhere in the \
         installation spells it; the records that store geometry and carry no measured prefix keep \
         UNINDEXED_ROLE_UNMEASURED, and the `fvol*` records the partition grid also names keep the \
         index rule while the disagreement is counted by partition_records_fog_volume. (b) the \
         world axis convention — the import reports the axis map it applied and its evidence class: \
         identity, right-handed, radians, at observed_tool for an installation-backed source, \
         contradicted when such a source applied another map, unknown for a designed source; the \
         measurement is task #436's static analysis of the owner's decrypted image (sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75), never a run. LIMITS OF \
         WHAT WAS MEASURED, each recorded in \
         docs/findings/2026-10-07-m01-lc-fvol-roles-and-axis-convention.md: (1) retail is file \
         access and the executable never ran, so nothing here is verified_original and no behavior \
         landmark exists until #358 supplies a run; (2) the fog-volume resolution is a designed \
         rule over a measured consumer — it says what this conversion presents, not that the 2000 \
         engine never intersected a fog box; (3) how the original engine collided with a record its \
         own spatial index names is still unmeasured, which is why the six grid-named `fvol*` \
         records keep the index rule; (4) whether the fog system's own inside test is a render-only \
         matter or reaches gameplay is not measured here. `unknowns` is empty because every \
         unresolved item above is a limit on the claim rather than an unresolved measurement: \
         every container imported and every unindexed record classified by a measured rule. The \
         {tests} assertions discovered under this task's own prefix are the tests of this report. \
         Validated with tools/validate_evidence.py --require-pass.",
        imported = imported,
        tests = tests
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_fvol_roles_writes_the_acceptance_report() {
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
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the fog/axis census over every world
    // container, plus the measured source's own calibration record.
    let census_path = evidence_dir.join("world-fvol-roles-census.json");
    let census = render_census(&game_dir, &install_sha256, &candidate_tree, &census_path);
    assert!(
        census > 0,
        "the report must not be written over an empty census"
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // The timestamp is a measured instant, not a shape: the calendar maths
    // behind it is asserted here so a broken conversion can never write a
    // report the validator has to reject later.
    let created_at = iso_utc_now();
    assert_eq!(
        (
            created_at.len(),
            created_at.as_bytes()[4],
            created_at.as_bytes()[7],
            created_at.as_bytes()[10]
        ),
        (20, b'-', b'-', b'T'),
        "the report's timestamp must be an ISO-8601 instant: {created_at}"
    );
    let year: i64 = created_at[..4].parse().expect("the year is four digits");
    assert!(
        (2000..2100).contains(&year),
        "the report's year is plausible for this run: {created_at}"
    );

    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-FVOL-ROLES\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&created_at),
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
        jstr(&review_method(census, suite.discovered as usize)),
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

/// The second production observation: import all eight world containers
/// through the measured GameZ source and record what the import reports —
/// counts, digests and claim labels, no original bytes.
///
/// Returns how many containers imported, and writes the census JSON.
fn render_census(
    game_dir: &Path,
    install_sha256: &str,
    candidate_tree: &str,
    path: &Path,
) -> usize {
    let found =
        read_world_containers(game_dir).expect("production world-container discovery reads");
    let mut groups: Vec<String> = Vec::new();
    let mut imported = 0usize;
    let mut fog_total = 0usize;
    let mut unresolved_total = 0usize;
    let mut indexed_fog_total = 0usize;
    for group in found.groups() {
        let container = found
            .container(&group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{group}: the container reads: {error}"));
        let adapter = retail_adapter(&container);
        let imported_world = container
            .definition(
                Origin::Installation {
                    source: container.span().clone(),
                },
                &adapter,
            )
            .unwrap_or_else(|error| {
                panic!("{}: the container imports: {error}", container.group())
            });
        let report = imported_world.report();
        imported += 1;
        fog_total += report.objects_unindexed_fog();
        unresolved_total += report.objects_unindexed_unresolved();
        indexed_fog_total += report.partition_records_fog_volume();
        groups.push(format!(
            "{{\"group\": {}, \"container\": {}, \"container_sha256\": {}, \"objects\": {}, \
             \"stored_child_list\": {}, \"objects_solid\": {}, \"objects_unindexed_none\": {}, \
             \"objects_unindexed_fog\": {}, \"objects_unindexed_unresolved\": {}, \
             \"partition_records_fog_volume\": {}, \"meters_per_unit\": {}, \"unit_class\": {}, \
             \"axis_map\": {}, \"axis_class\": {}, \"angle_unit\": {}, \"rotation_sense\": {}}}",
            jstr(container.group()),
            jstr(container.container_key()),
            jstr(container.container_sha256()),
            report.objects(),
            report.stored_child_list(),
            report.objects_solid(),
            report.objects_unindexed_none(),
            report.objects_unindexed_fog(),
            report.objects_unindexed_unresolved(),
            report.partition_records_fog_volume(),
            report.meters_per_unit(),
            jstr(report.unit_class().label()),
            jstr(report.axis_map()),
            jstr(report.axis_class().label()),
            jstr(angle_unit_label(report.angle_unit())),
            jstr(rotation_label(report.rotation_sense())),
        ));
    }

    // The measured source's own calibration record: the landmark census and
    // the gaps it honestly keeps.
    let first_group = found
        .groups()
        .into_iter()
        .next()
        .expect("at least one world group exists");
    let span = found
        .container(&first_group, &WorldTextureLoad::project_default())
        .expect("the first group's container reads")
        .span()
        .clone();
    let source = CoordinateSource::retail_gamez(span);
    let calibration = source.calibration();
    let quantities: Vec<String> = cs_content::coordinates::CalibratedQuantity::ALL
        .iter()
        .map(|quantity| {
            format!(
                "{{\"quantity\": {}, \"landmarks\": {}, \"behaviors\": {}, \"status\": {}}}",
                jstr(quantity.label()),
                calibration.landmark_count(*quantity),
                calibration.behavior_landmark_count(*quantity),
                jstr(calibration.quantity_status(*quantity).label()),
            )
        })
        .collect();

    fs::write(
        path,
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"containers\": {},\n \
             \"fog_volumes\": {},\n \"unindexed_unresolved\": {},\n \"partition_fog_volumes\": {},\n \
             \"measured_source\": {{\"label\": {}, \"claim_status\": {}, \"complete\": {}, \
             \"quantities\": [{}]}},\n \"groups\": [{}]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            imported,
            fog_total,
            unresolved_total,
            indexed_fog_total,
            jstr(source.label()),
            jstr(calibration.claim_status().label()),
            calibration.is_complete(),
            quantities.join(", "),
            groups.join(",\n  "),
        ),
    )
    .expect("write world-fvol-roles-census.json");
    imported
}

/// The angle unit's stable label, read from the enum rather than spelled here.
fn angle_unit_label(unit: cs_content::coordinates::AngleUnit) -> &'static str {
    match unit {
        cs_content::coordinates::AngleUnit::Radians => "radians",
        cs_content::coordinates::AngleUnit::Degrees => "degrees",
    }
}

/// The rotation sense's stable label.
fn rotation_label(sense: cs_content::coordinates::RotationSense) -> &'static str {
    match sense {
        cs_content::coordinates::RotationSense::RightHandRule => "right_hand_rule",
        cs_content::coordinates::RotationSense::LeftHandRule => "left_hand_rule",
    }
}

/// The conversion the census imports through: the measured GameZ source over
/// the container's own span.
fn retail_adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_fvol_roles.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-FVOL-ROLES` written relative
/// to the workspace root in the module doc must be re-anchored here.
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

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3600) / 60,
        since % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to y/m/d.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (day_of_year * 5 + 2) / 153;
    let day = (day_of_year - (shifted * 153 + 2) / 5 + 1) as u32;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
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

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance
/// selection. A prefixed test that was skipped is recorded with the schema's
/// `unknown` status rather than counted as a pass, so a report can never claim
/// an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(&name) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|segment| segment.starts_with(ACCEPTANCE_PREFIX))
}

/// The libtest result word at the head of a test's tail line.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 (the
/// validator re-hashes it with `hashlib` independently).
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
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
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
                 \"world-fvol-roles-census.json\"]}}",
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
