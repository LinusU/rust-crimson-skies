//! Evidence-report harness for task `M01-LC-SCENARIOS`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_scenarios_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_lc_scenarios_ --include-ignored 2>&1 |
//!      tee private/evidence/M01-LC-SCENARIOS/cargo-test.log
//!    ```
//!    (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-SCENARIOS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_scenarios_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_scenarios -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/M01-LC-SCENARIOS/acceptance.json \
//!      --artifact-root private/evidence/M01-LC-SCENARIOS --require-pass
//!    ```
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-SCENARIOS.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_scenario_control_programs`] and records the census — every scenario
//! row, its members, the descriptor the contents rule selected with its full
//! field vocabulary, the record-level objective keys and their classification,
//! the measured `net.zrd` shape and the target markers — as JSON. That is a
//! real production run over the owner's installation, not a paraphrase of the
//! acceptance assertions, and it carries no original bytes: ids, digests, byte
//! extents, spellings, shapes and counts only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::scenario_control::{
    RetailScenarioCensus, RetailScenarioRow, ScenarioProgram, survey_scenario_control_programs,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::scenario_control::NetSpawnSurface;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_scenarios_";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from
/// [`survey_scenario_control_programs`] rather than written down, so a report
/// regenerated on another installation cannot describe this one's numbers, and
/// the limits of the claim are stated beside them.
fn review_method(census: &RetailScenarioCensus, own_tests: usize) -> String {
    let descriptors = census.descriptor_rows().count();
    let without = census.archives_without_descriptor();
    let unclassified = census.unclassified_record_keys();
    let net_shapes: Vec<String> = census
        .net_readers()
        .iter()
        .map(|(mission, surface)| {
            let shape = match surface {
                NetSpawnSurface::Absent => "absent".to_owned(),
                NetSpawnSurface::Measured(count) => format!("{count} entries"),
                NetSpawnSurface::UnmeasuredShape => "unmeasured shape".to_owned(),
            };
            format!("{mission}: {shape}")
        })
        .collect();
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives \
         every field from the recorded log, production discovery of $CS_GAME_DIR and a \
         second production run of cs_app::scenario_control::\
         survey_scenario_control_programs over the installation \
         (scenario-control-census.json). Claim is implemented only. MEASURED: every \
         mission-scoped reader archive was opened through production discovery, every \
         member decoded with the production .zrd reader, and each archive declaring no \
         numbered OBJECTIVE<N> block located its control member by a contents rule — \
         the unique member whose decoded record declares the measured scenario \
         descriptor (a mission_type field, F42-A's grammar) — with zero candidates a \
         measured absence and two a refusal. Of {total} mission-scoped readers, \
         {blocks} carry a block-bearing control member (the mission population, \
         counted but not re-measured) and {scenarios} are scenarios: {descriptors} \
         declare exactly one descriptor member and {absent} declare none ({without}) — \
         the multiplayer readers, whose control data is the different, thinner surface \
         this census measures beside the absence: the record-level objectives.zrd keys \
         (vocabulary {vocabulary}; spellings outside the measured record vocabulary: \
         {unclassified}), the bare net.zrd spawn-point list (every net.zrd in the \
         installation parses as the measured four-float shape: {net_shapes}) and the \
         objective-marked targets.zrd records. No campaign-mission reader anywhere \
         declares a descriptor member ({sites}). LIMITS OF WHAT WAS MEASURED, each \
         recorded in docs/findings/2026-10-13-m01-lc-scenarios.md: (1) what the files \
         DECLARE is measured and what any declaration DOES is not — no original \
         executable has been run, so a scenario mode, a spawn coordinate and an \
         objective marker are spellings, never observed behaviour; (2) the fourth \
         number of a spawn entry, the meaning of a scenario mode and the rule by which \
         a multiplayer objective completes are unmeasured; (3) zeppelins.zrd records \
         are counted only — their field vocabulary stays F35's and T574's measurement; \
         (4) an instant-action or multiplayer scenario is not a campaign mission: this \
         census never widens the mission denominator and lowers nothing to a \
         MissionProgram. `unknowns` is empty because every unresolved item above is a \
         limit on the claim rather than an unresolved measurement: every archive either \
         located its control surface or was named as declaring none. TEST-SELECTION \
         NOTE: the prefix {prefix} is unique to this task's file, so this run's \
         {own_tests} discovered assertions are exactly this task's.",
        total = census.len() + census.block_bearing_readers() as usize,
        blocks = census.block_bearing_readers(),
        scenarios = census.len(),
        absent = without.len(),
        without = if without.is_empty() {
            "none".to_owned()
        } else {
            without.join(", ")
        },
        vocabulary = census
            .record_key_vocabulary()
            .iter()
            .map(|(key, rows)| format!("{key} ({rows} row(s))"))
            .collect::<Vec<_>>()
            .join(", "),
        unclassified = if unclassified.is_empty() {
            "none".to_owned()
        } else {
            unclassified.join(", ")
        },
        net_shapes = net_shapes.join("; "),
        sites = if census.descriptor_sites_outside_scenarios().is_empty() {
            "is empty".to_owned()
        } else {
            format!("names {:?}", census.descriptor_sites_outside_scenarios())
        },
        prefix = ACCEPTANCE_PREFIX,
        own_tests = own_tests,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_scenarios_writes_the_acceptance_report() {
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

    // The second production observation: the scenario census, rendered per
    // archive with every measured surface.
    let census = survey_scenario_control_programs(&game_dir).expect("the scenario census surveys");
    assert!(
        !census.is_empty(),
        "the report must not be written over an empty census"
    );
    let census_path = evidence_dir.join("scenario-control-census.json");
    fs::write(
        &census_path,
        format!(
            "{}\n",
            render_census(&census, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write scenario-control-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-SCENARIOS\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&census, own_tests(&log))),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT \
         validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The census as JSON: hashes, byte extents, key spellings, shapes and counts.
/// No original bytes and no original text statements.
fn render_census(
    census: &RetailScenarioCensus,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let rows: Vec<String> = census.rows().iter().map(render_row).collect();
    let vocabulary: Vec<String> = census
        .record_key_vocabulary()
        .iter()
        .map(|(key, rows)| format!("{{\"key\": {}, \"rows\": {rows}}}", jstr(key)))
        .collect();
    let unclassified: Vec<String> = census
        .unclassified_record_keys()
        .iter()
        .map(|key| jstr(key))
        .collect();
    let net_readers: Vec<String> = census
        .net_readers()
        .iter()
        .map(|(mission, surface)| {
            format!(
                "{{\"mission\": {}, \"surface\": {}}}",
                jstr(mission),
                render_net(surface)
            )
        })
        .collect();
    let sites: Vec<String> = census
        .descriptor_sites_outside_scenarios()
        .iter()
        .map(|site| jstr(site))
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"scenario_rows\": {}, \
         \"block_bearing_readers\": {}, \"descriptor_sites_outside_scenarios\": [{}], \
         \"archives_without_descriptor\": [{}], \"net_readers\": [{}], \
         \"record_key_vocabulary\": [{}], \"unclassified_record_keys\": [{}], \
         \"rows\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        census.len(),
        census.block_bearing_readers(),
        sites.join(", "),
        str_array(
            &census
                .archives_without_descriptor()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        ),
        net_readers.join(", "),
        vocabulary.join(", "),
        unclassified.join(", "),
        rows.join(", "),
    )
}

/// One scenario row: its members, the selected descriptor with its measured
/// vocabulary and the thinner surfaces beside it.
fn render_row(row: &RetailScenarioRow) -> String {
    let members: Vec<String> = row
        .members
        .iter()
        .map(|member| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}}}",
                jstr(&member.name),
                member.offset,
                member.len,
                member.objective_blocks,
            )
        })
        .collect();
    let program = match &row.program {
        ScenarioProgram::Descriptor {
            member,
            offset,
            len,
            sha256,
            descriptor,
        } => {
            let fields: Vec<String> = descriptor
                .field_shapes()
                .iter()
                .map(|(key, shape)| {
                    format!(
                        "{{\"key\": {}, \"shape\": {}}}",
                        jstr(key),
                        jstr(&shape.label())
                    )
                })
                .collect();
            let spawn_modes: Vec<String> = descriptor
                .spawn_modes()
                .iter()
                .map(|(mode, entries)| {
                    format!("{{\"mode\": {}, \"entries\": {entries}}}", jstr(mode))
                })
                .collect();
            let zones: Vec<String> = descriptor
                .zone_bindings()
                .iter()
                .map(|(node, label)| {
                    format!("{{\"node\": {}, \"label\": {}}}", jstr(node), jstr(label))
                })
                .collect();
            let groups: Vec<String> = descriptor
                .aircraft()
                .enemy_groups()
                .iter()
                .map(|group| {
                    format!(
                        "{{\"index\": {}, \"plane\": {}, \"skill\": {}}}",
                        group.index(),
                        jstr(group.plane().unwrap_or("")),
                        jstr(group.skill().unwrap_or("")),
                    )
                })
                .collect();
            format!(
                "{{\"label\": \"descriptor\", \"member\": {}, \"offset\": {offset}, \"length\": \
                 {len}, \"member_sha256\": {}, \"mission_type\": {}, \"field_shapes\": [{}], \
                 \"spawn_modes\": [{}], \"zone_bindings\": [{}], \"player_plane\": {}, \
                 \"wingmen\": {}, \"enemy_groups\": [{}]}}",
                jstr(member),
                jstr(sha256),
                jstr(descriptor.mission_type().unwrap_or("")),
                fields.join(", "),
                spawn_modes.join(", "),
                zones.join(", "),
                jstr(descriptor.aircraft().player_plane().unwrap_or("")),
                descriptor
                    .aircraft()
                    .wingmen()
                    .map_or_else(|| "null".to_owned(), |count| count.to_string()),
                groups.join(", "),
            )
        }
        ScenarioProgram::NoDescriptor { scanned } => {
            format!("{{\"label\": \"no-descriptor\", \"scanned_members\": {scanned}}}")
        }
    };
    let record_objective = match &row.record_objective {
        Some(record) => {
            let keys: Vec<String> = record
                .keys()
                .iter()
                .map(|(key, sites)| format!("{{\"key\": {}, \"sites\": {sites}}}", jstr(key)))
                .collect();
            let unclassified: Vec<String> =
                record.unclassified().iter().map(|key| jstr(key)).collect();
            format!(
                "{{\"keys\": [{}], \"unclassified\": [{}]}}",
                keys.join(", "),
                unclassified.join(", "),
            )
        }
        None => "null".to_owned(),
    };
    let targets = row.targets.as_ref().map_or_else(
        || "null".to_owned(),
        |targets| {
            format!(
                "{{\"records\": {}, \"objective_marked\": {}, \"other_target_marked\": {}}}",
                targets.records, targets.objective_marked, targets.other_target_marked,
            )
        },
    );
    format!(
        "{{\"mission\": {}, \"container\": {}, \"container_sha256\": {}, \"members\": [{}], \
         \"program\": {}, \"record_objective\": {}, \"net_spawns\": {}, \"targets\": {}, \
         \"zeppelin_records\": {}}}",
        jstr(&row.mission),
        jstr(&row.container),
        jstr(&row.container_sha256),
        members.join(", "),
        program,
        record_objective,
        render_net(&row.net_spawns),
        targets,
        row.zeppelin_records
            .map_or_else(|| "null".to_owned(), |records| records.to_string()),
    )
}

fn render_net(surface: &NetSpawnSurface) -> String {
    match surface {
        NetSpawnSurface::Absent => "\"absent\"".to_owned(),
        NetSpawnSurface::Measured(count) => {
            format!("{{\"label\": \"measured\", \"entries\": {count}}}")
        }
        NetSpawnSurface::UnmeasuredShape => "\"unmeasured-shape\"".to_owned(),
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its \
             module doc (crates/cs_app/tests/evidence_report_m01_lc_scenarios.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path written relative to the workspace root in the module doc
/// must be re-anchored here.
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

/// Extracts the per-test results of the prefixed tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with the
/// schema's `unknown` status rather than counted as a pass, so a report can
/// never claim an assertion it did not run.
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

/// Whether a libtest name is one of this task's tests. The prefix is matched
/// on the name's **last** path segment, so a future in-module test with the
/// prefix is never silently dropped.
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

/// How many of the discovered prefixed assertions belong to **this** task's
/// file, counted from the recorded log (libtest prints an integration-test
/// file's tests with no module path).
fn own_tests(log: &str) -> usize {
    parse_suite(log)
        .assertions
        .iter()
        .filter(|(name, _)| !name.contains("::"))
        .count()
}

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it independently).
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
                 \"scenario-control-census.json\"]}}",
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

/// Keeps the census types in the harness's signature honest: the report reads
/// only what these accessors expose.
const _: fn(&RetailScenarioCensus) -> &[RetailScenarioRow] = RetailScenarioCensus::rows;
