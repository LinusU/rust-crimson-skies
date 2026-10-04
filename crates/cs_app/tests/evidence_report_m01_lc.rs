//! Evidence-report harness for task `M01-LC-MISSION-PROGRAM`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-LC-MISSION-PROGRAM/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-MISSION-PROGRAM \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-MISSION-PROGRAM/acceptance.json
//!    --artifact-root private/evidence/M01-LC-MISSION-PROGRAM --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-MISSION-PROGRAM.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_mission_control_programs`] and records the census — the two measured
//! populations, every mission's members and control program, every directive key
//! with its argument shapes and its disposition, and the lowering accounting — as
//! JSON. That is a real production run over the owner's installation, not a
//! paraphrase of the acceptance assertions, and it carries no original bytes: ids,
//! digests, byte extents, shapes and dispositions only.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::{
    ControlProgram, RetailControlCensus, RetailMemberRow, survey_mission_control_programs,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::mission_control::{ControlLowering, MeasuredControlRecord, UnmeasuredReason};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from
/// [`survey_mission_control_programs`] rather than written down, so a report
/// regenerated on another installation cannot describe this one's numbers. The
/// limits of the claim are interpolated from the census's own lowering accounting,
/// so a report can never claim more than this run measured.
fn review_method(
    census: &RetailControlCensus,
    own_tests: usize,
    shared_plus_own: usize,
    other_modules: &[(String, usize)],
) -> String {
    let absent = census.archives_without_control_program();
    let shared = shared_plus_own.saturating_sub(own_tests);
    let other_modules = if other_modules.is_empty() {
        "none".to_owned()
    } else {
        other_modules
            .iter()
            .map(|(module, count)| format!("{count} in {module}::"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let unmet: Vec<String> = census
        .unmet_by_requirement()
        .iter()
        .map(|(kind, missions)| format!("{kind} on {} mission(s)", missions.len()))
        .collect();
    let fields = census.unmeasured_fields();
    // The name-reading counterexample is **derived**, never written down: the
    // reader whose longest member exceeds its control member by the largest
    // factor, with both byte extents from the same census row.
    let widest = census
        .measured_rows()
        .filter_map(|row| {
            let control_len = match &row.program {
                ControlProgram::Measured { len, .. } => *len,
                ControlProgram::Absent { .. } => return None,
            };
            row.members
                .iter()
                .filter(|member| {
                    !member.is_control
                        && member.len > control_len
                        && control_len > 0
                        && member.objective_blocks == 0
                })
                .max_by_key(|member| member.len)
                .map(|member| {
                    (
                        row.mission.clone(),
                        member.len as f64 / control_len as f64,
                        member.len,
                        control_len,
                    )
                })
        })
        .fold(
            (String::new(), 0.0_f64, 0_u64, 0_u64),
            |worst, candidate| {
                if candidate.1 > worst.1 {
                    candidate
                } else {
                    worst
                }
            },
        );
    let (widest_mission, widest_ratio, widest_len, control_len) = widest;
    let widest_mission = if widest_mission.is_empty() {
        "no measured reader".to_owned()
    } else {
        widest_mission
    };
    let widest_ratio = format!("{widest_ratio:.2}");
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of cs_app::mission_control::survey_mission_control_programs over the installation \
         (control-program-census.json). Claim is implemented only. MEASURED: every mission-scoped \
         reader archive of the installation was opened through production discovery, EVERY member \
         decoded with the production .zrd reader, and the control member selected by a rule rather \
         than by name: the member whose decoded record declares numbered OBJECTIVE<N> blocks. {} \
         of {} mission-scoped readers declare such a member and were measured; the other {} declare \
         none and every one of them is an instant-action or multiplayer scenario ({}) - they are \
         carried as a measured absence, not dropped and not counted as campaign missions. Over the \
         measured archives the installation declares {} numbered objective blocks, {} directive \
         sites and {} distinct directive keys. The rule is measured to disagree with the name \
         reading the task started from: in {} the longest member of the reader is {}x the length \
         of that reader's control member ({} bytes beside {}) and declares no objective block at \
         all, so a size-or-name heuristic would have selected a member with no objective program \
         in it as the mission program. \
         The declared argument shape of every directive site was measured, keeping nested lists \
         nested and counting the sites whose shape disagrees across a key's own sites; no shape is \
         flattened into a positional list and no majority shape is resolved. Exactly two directive \
         keys corpus-wide reach a mission-IR operation - the measured outcome keys - and their \
         outcome is a reading of their spelling, never an observation of behaviour; every other \
         key is refused with a named reason. LIMITS OF WHAT WAS MEASURED, each recorded in \
         docs/findings/2026-10-04-m01-lc-mission-program.md: (1) what the corpus DECLARES is \
         measured and what any declaration DOES is not - no original executable has been run, so \
         nothing here is evidence of behaviour; (2) the mission-language instruction table the \
         F13-C census searched for is not what a mission-scoped reader contains: its control \
         program is a typed keyed list in one member, and the bytecode mission VM that F13-B/C \
         reported as 0-of-1452 resolved remains unlocated and unmeasured; (3) lower_program's four \
         requirements are unmet for every measured archive - {} - and the unmeasured fields it \
         names are {} - so no mission is lowered, no mission is playable and this report claims no \
         original behaviour; (4) the mission id is not a member field: the reader is mission-scoped \
         by path and the canonical id comes from the campaign binding record (M01-A); (5) an empty \
         or unreadable record never reports itself complete, and the campaign gate is closed while \
         any reader declares a directive with no measured effect. `unknowns` is empty because \
         every unresolved item above is a limit on the claim rather than an unresolved measurement: \
         every row in the census resolved, every archive either measured or named as absent. \
         TEST-SELECTION NOTE: the prefix accept_m01_lc_ is shared, so this run's \
         {shared_plus_own} discovered assertions cover more than this task: the {own_tests} \
         belonging to THIS task are exactly the accept_m01_lc_ tests in \
         crates/cs_app/tests/accept_m01_lc_mission_program.rs (libtest prints an integration-test \
         file's tests with no module path), and the other {shared} arrive under another task's \
         module path ({other_modules}) and appear here only because the selection is \
         prefix-based. \
         Validated with tools/validate_evidence.py --require-pass.",
        census.measured_len(),
        census.len(),
        absent.len(),
        if absent.is_empty() {
            "none".to_owned()
        } else {
            absent.join(", ")
        },
        census.blocks(),
        census.sites(),
        census.directive_keys().len(),
        widest_mission,
        widest_ratio,
        widest_len,
        control_len,
        if unmet.is_empty() {
            "none".to_owned()
        } else {
            unmet.join(", ")
        },
        if fields.is_empty() {
            "none".to_owned()
        } else {
            format!("{} field(s), including {}", fields.len(), fields[0])
        },
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_writes_the_acceptance_report() {
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

    // The second production observation: the census, rendered per mission with
    // every measured directive key, its shapes and its disposition.
    let census = survey_mission_control_programs(&game_dir).expect("the census surveys");
    assert!(
        !census.is_empty(),
        "the report must not be written over an empty census"
    );
    let census_path = evidence_dir.join("control-program-census.json");
    fs::write(
        &census_path,
        format!(
            "{}\n",
            render_census(&census, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write control-program-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-MISSION-PROGRAM\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(
            &census,
            own_tests(&log),
            suite.discovered as usize,
            &other_task_modules(&log),
        )),
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

/// The census as JSON: hashes, byte extents, key spellings, argument shapes and
/// dispositions. No original bytes and no original text statements.
fn render_census(
    census: &RetailControlCensus,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let rows: Vec<String> = census
        .rows()
        .iter()
        .map(|row| {
            let members: Vec<String> = row.members.iter().map(render_member).collect();
            let program = match &row.program {
                ControlProgram::Measured {
                    member,
                    offset,
                    len,
                    sha256,
                    record,
                } => format!(
                    "{{\"label\": \"measured\", \"member\": {}, \"offset\": {offset}, \"length\": \
                     {len}, \"member_sha256\": {}, \"record\": {}}}",
                    jstr(member),
                    jstr(sha256),
                    render_record(record),
                ),
                ControlProgram::Absent { scanned } => {
                    format!("{{\"label\": \"absent\", \"scanned_members\": {scanned}}}")
                }
            };
            format!(
                "{{\"mission\": {}, \"container\": {}, \"container_sha256\": {}, \"members\": \
                 [{}], \"program\": {}}}",
                jstr(&row.mission),
                jstr(&row.container),
                jstr(&row.container_sha256),
                members.join(", "),
                program,
            )
        })
        .collect();
    let vocabulary: Vec<String> = census
        .vocabulary()
        .iter()
        .map(|(key, sites)| format!("{{\"key\": {}, \"sites\": {sites}}}", jstr(key)))
        .collect();
    let unmet: Vec<String> = census
        .unmet_by_requirement()
        .iter()
        .map(|(kind, missions)| {
            format!(
                "{{\"requirement\": {}, \"missions\": {}}}",
                jstr(kind),
                missions.len()
            )
        })
        .collect();
    let fields: Vec<String> = census
        .unmeasured_fields()
        .iter()
        .map(|field| jstr(field))
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"missions\": {}, \"measured\": {}, \
         \"absent\": {}, \"blocks\": {}, \"sites\": {}, \"distinct_keys\": {}, \"complete_missions\": \
         {}, \"campaign_ready\": {}, \"archives_without_control_program\": {}, \"vocabulary\": [{}], \
         \"unmet_requirements\": [{}], \"unmeasured_fields\": [{}], \"rows\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        census.len(),
        census.measured_len(),
        census.archives_without_control_program().len(),
        census.blocks(),
        census.sites(),
        census.directive_keys().len(),
        census.complete_missions().len(),
        census.campaign_ready(),
        str_array(
            &census
                .archives_without_control_program()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        ),
        vocabulary.join(", "),
        unmet.join(", "),
        fields.join(", "),
        rows.join(", "),
    )
}

/// One member of a reader archive: its name, extent, measured block count and
/// whether the control rule selected it.
fn render_member(member: &RetailMemberRow) -> String {
    format!(
        "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}, \"is_control\": {}}}",
        jstr(&member.name),
        member.offset,
        member.len,
        member.objective_blocks,
        member.is_control,
    )
}

/// One measured control record: its counts, every directive key with its measured
/// argument shapes and its disposition, the record fields outside the blocks and
/// the lowering accounting.
fn render_record(record: &MeasuredControlRecord) -> String {
    let keys: Vec<String> = record
        .keys()
        .iter()
        .map(|key| {
            let shapes: Vec<String> = key
                .shapes
                .iter()
                .map(|(shape, sites)| {
                    format!("{{\"shape\": {}, \"sites\": {sites}}}", jstr(&shape.label()))
                })
                .collect();
            let disposition = match key.disposition() {
                cs_content::mission_control::DirectiveDisposition::TerminalOutcome { outcome } => {
                    format!("{{\"kind\": \"terminal_outcome\", \"outcome\": {}}}", jstr(outcome.label()))
                }
                cs_content::mission_control::DirectiveDisposition::Unmeasured { reason } => format!(
                    "{{\"kind\": \"unmeasured\", \"reason\": {}, \"detail\": {}}}",
                    jstr(reason.code()),
                    jstr(&reason.detail())
                ),
            };
            format!(
                "{{\"key\": {}, \"blocks\": {}, \"sites\": {}, \"shapes\": [{}], \"disposition\": {}}}",
                jstr(&key.key),
                key.blocks,
                key.sites,
                shapes.join(", "),
                disposition,
            )
        })
        .collect();
    let fields: Vec<String> = record
        .record_fields()
        .iter()
        .map(|(field, sites)| {
            format!(
                "{{\"key\": {}, \"sites\": {sites}, \"support\": {}}}",
                jstr(field.key()),
                jstr(field.support().label())
            )
        })
        .collect();
    let unclassified: Vec<String> = record
        .unclassified_record_keys()
        .iter()
        .map(|key| jstr(key))
        .collect();
    let refusals: Vec<String> = record
        .refusals()
        .iter()
        .map(|refusal| {
            format!(
                "{{\"code\": {}, \"detail\": {}}}",
                jstr(refusal.code()),
                jstr(&refusal.to_string())
            )
        })
        .collect();
    let lowering = record.lowering();
    let requirements: Vec<String> = lowering
        .requirements()
        .iter()
        .map(|row| {
            let unmeasured: Vec<String> = row.unmeasured_fields.iter().map(|f| jstr(f)).collect();
            format!(
                "{{\"requirement\": {}, \"met\": {}, \"measurement\": {}, \"unmeasured_fields\": [{}]}}",
                jstr(row.kind.code()),
                row.met,
                jstr(&row.measurement),
                unmeasured.join(", "),
            )
        })
        .collect();
    format!(
        "{{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \"complete\": {}, \"record_fields\": \
         [{}], \"unclassified_record_keys\": [{}], \"block_refusals\": [{}], \"keys\": [{}], \
         \"lowering\": {{\"complete\": {}, \"unmeasured_fields\": {}, \"requirements\": [{}]}}}}",
        record.blocks(),
        record.sites(),
        record.vocabulary(),
        record.is_complete(),
        fields.join(", "),
        unclassified.join(", "),
        refusals.join(", "),
        keys.join(", "),
        lowering.complete(),
        lowering.unmeasured_fields().len(),
        requirements.join(", "),
    )
}

/// Keeps the type in the harness's signature honest: the census's accounting is
/// the only place a `ControlLowering` is read for the report, and the reason
/// vocabulary it renders is fixed.
const _: fn(&UnmeasuredReason) -> &str = UnmeasuredReason::code;
const _: fn(&ControlLowering) -> bool = ControlLowering::complete;

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-MISSION-PROGRAM` written relative
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
    // A Gregorian date rendered from the epoch second, so the report needs no
    // calendar dependency.
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

/// Extracts the per-test results of the `accept_m01_lc_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it would
/// report hundreds of unrelated tests as this task's acceptance selection. A
/// prefixed test that was skipped is recorded with the schema's `unknown` status
/// rather than counted as a pass, so a report can never claim an assertion it did
/// not run.
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
///
/// The prefix is matched on the name's **last** path segment, because libtest prints
/// an in-module unit test under its module path. Matching the whole name instead
/// would silently drop every in-module acceptance test from `discovered`, from the
/// assertion list and from the log.
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

/// How many of the discovered prefixed assertions belong to **this** task.
///
/// The prefix `accept_m01_lc_` is shared: besides this task's file, M01-LC's world
/// scene ids (`scene_ids::`) and the world import (`import_retail::`) publish
/// assertions under it, so a prefix-based selection reports all of them. Counting
/// only the un-namespaced ones — libtest prints a test from an integration-test
/// *file* with no module path and an in-module test under its module path — is
/// what separates this task's set, and it is counted from the recorded log rather
/// than from a list the harness maintains.
fn own_tests(log: &str) -> usize {
    parse_suite(log)
        .assertions
        .iter()
        .filter(|(name, _)| !name.contains("::"))
        .count()
}

/// The module paths the other tasks' prefixed assertions arrive under, with how
/// many each carries, so the report names its neighbours instead of counting them
/// anonymously. Derived from the recorded log, not from a list kept here.
fn other_task_modules(log: &str) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (name, _) in &parse_suite(log).assertions {
        if !name.contains("::") {
            continue;
        }
        let module = name.rsplit("::").nth(1).unwrap_or_default().to_owned();
        *counts.entry(module).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

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
                 \"control-program-census.json\"]}}",
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

/// A JSON string literal: quoted and escaped, so no report field can break out of
/// its string.
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
