//! Evidence-report harness for task `M01-LC-WORLD-UNIT-ROLES`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_world_unit_roles_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_world_unit_roles
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-WORLD-UNIT-ROLES/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-WORLD-UNIT-ROLES \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_world_unit_roles --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_world_unit_roles -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-WORLD-UNIT-ROLES/acceptance.json
//!    --artifact-root private/evidence/M01-LC-WORLD-UNIT-ROLES --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs the import over all
//! eight world containers and records the census — the per-container unindexed
//! split, the unit factor and its evidence class, and the measured source's own
//! calibration gaps — as JSON. That is a real production run over the owner's
//! installation, not a paraphrase of the acceptance assertions, and it carries
//! no original bytes: ids, digests, counts and claim labels only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::world::{RetailWorldContainer, read_world_containers};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::coordinates::{CalibratedQuantity, CoordinateSource, SourceAdapter};
use cs_types::content::Origin;

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_world_unit_roles_";

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
         of the world-container import path over the installation (world-unit-roles-census.json). \
         Claim is implemented only. MEASURED: (a) the GameZ stored-vertex unit — the landmark \
         census recorded on CoordinateSource::retail_gamez's own calibration, re-observed here, \
         pins the scale quantity to one stored unit per metre at observed_tool: all 61 animation \
         containers store the f32 -9.8 gravity word, the pilot-figure and airframe subtrees measure \
         their known real-world sizes only at the metre, the LOD switch bands read as metres, and \
         the world bounds corroborate; the axis map, handedness and angle unit are unmeasured and \
         the source's gap list says so, so no whole-convention claim is made and nothing is \
         verified_original. (b) the unindexed-record split — over all {imported} world containers, \
         every record the partition grid omits stores either a mesh index and a non-zero bounding \
         box or neither of them; the no-geometry half resolves to WorldCollisionRole::None \
         (UNINDEXED_RECORD_STORES_NO_GEOMETRY, the store gives the record nothing a collider could \
         be built from) and the geometry-bearing half keeps an explicit unknown \
         (UNINDEXED_ROLE_UNMEASURED, the container never says whether the engine collided with the \
         fvol* volumes). The spawn's own report was checked: anchors are presented and never block \
         and are not skips, volumes report unknown_collision_role, and no substitute geometry was \
         introduced. LIMITS OF WHAT WAS MEASURED, each recorded in \
         docs/findings/2026-10-05-m01-lc-world-unit-roles.md: (1) retail is file access — no \
         original executable ran, so the metre claim is a byte census, not an observation of \
         original behaviour, and it never reaches verified_original without fingerprinted \
         original-run evidence; (2) the None resolution is about the record's OWN stored geometry \
         only — the content parked under an anchor (the airship a *zep record parents, the \
         instances a g* group scatters) is other records' business and is measured by the actor \
         and animation surfaces, not here; (3) which fvol* volumes are sensors, triggers or \
         decoration is still unmeasured and stays an explicit unknown — M01's mission data names \
         none of them; (4) the axis map, handedness, angle unit, rotation sense and front-face \
         rule of the GameZ convention are declared, not measured — the world import's \
         unit_class accessor reports the scale quantity's class only. `unknowns` is empty because \
         every unresolved item above is a limit on the claim rather than an unresolved \
         measurement: every container imported, every unindexed record classified by its own \
         stored fields. The {tests} assertions discovered under this task's own prefix are the \
         tests of this report. Validated with tools/validate_evidence.py --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_world_unit_roles_writes_the_acceptance_report() {
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

    // The second production observation: the split census over every world
    // container, plus the measured source's own calibration record.
    let census_path = evidence_dir.join("world-unit-roles-census.json");
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
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-WORLD-UNIT-ROLES\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
    for group in found.groups() {
        let container = found
            .container(&group)
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
        groups.push(format!(
            "{{\"group\": {}, \"container\": {}, \"container_sha256\": {}, \"objects\": {}, \
             \"stored_child_list\": {}, \"objects_solid\": {}, \"objects_unindexed_none\": {}, \
             \"objects_unindexed_unresolved\": {}, \"objects_resident\": {}, \"sectors\": {}, \
             \"sectors_without_extent\": {}, \"empty_cells\": {}, \"meters_per_unit\": {}, \
             \"unit_class\": {}}}",
            jstr(container.group()),
            jstr(container.container_key()),
            jstr(container.container_sha256()),
            report.objects(),
            report.stored_child_list(),
            report.objects_solid(),
            report.objects_unindexed_none(),
            report.objects_unindexed_unresolved(),
            report.objects_resident(),
            report.sectors(),
            report.sectors_without_extent(),
            report.empty_cells(),
            report.meters_per_unit(),
            jstr(report.unit_class().label()),
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
        .container(&first_group)
        .expect("the first group's container reads")
        .span()
        .clone();
    let source = CoordinateSource::retail_gamez(span);
    let calibration = source.calibration();
    let gaps: Vec<String> = calibration
        .gaps()
        .iter()
        .map(|gap| {
            format!(
                "{{\"quantity\": {}, \"landmarks_recorded\": {}, \"landmarks_required\": {}, \
                 \"behavior_landmarks\": {}}}",
                jstr(gap.quantity.label()),
                gap.landmarks_recorded,
                gap.landmarks_required,
                gap.behavior_landmarks,
            )
        })
        .collect();
    let quantities: Vec<String> = CalibratedQuantity::ALL
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
             \"measured_source\": {{\"label\": {}, \"claim_status\": {}, \"complete\": {}, \
             \"quantities\": [{}], \"gaps\": [{}]}},\n \"groups\": [{}]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            imported,
            jstr(source.label()),
            jstr(calibration.claim_status().label()),
            calibration.is_complete(),
            quantities.join(", "),
            gaps.join(", "),
            groups.join(",\n  "),
        ),
    )
    .expect("write world-unit-roles-census.json");
    imported
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
             (crates/cs_app/tests/evidence_report_m01_lc_world_unit_roles.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-WORLD-UNIT-ROLES` written
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

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
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
                 \"world-unit-roles-census.json\"]}}",
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
