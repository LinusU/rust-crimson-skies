//! Evidence-report harness for task `M01-LC-DIRECTIVE-LOWERING` (#717, stage
//! `.03` = #726): `docs/contracts/CLI-EVIDENCE.md`, schema
//! `schemas/evidence.schema.json`. Not named `accept_m01_lc_lowering_adapter_*`:
//! it is not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_
//!    accept_m01_lc_directive_lowering_ --include-ignored 2>&1 | tee
//!    private/evidence/M01-LC-DIRECTIVE-LOWERING/cargo-test.log` (note the exit
//!    status; libtest ORs the two filters, and both prefixes are unique to this
//!    task — stage `.03`'s adapter suite and the parent's own integration suite)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVE-LOWERING \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_ accept_m01_lc_directive_lowering_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_directive_lowering -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-DIRECTIVE-LOWERING/acceptance.json
//!    --artifact-root private/evidence/M01-LC-DIRECTIVE-LOWERING --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-DIRECTIVE-LOWERING.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_mission_control_programs`] over the installation and records, for
//! every mission row, whether its own lowering attempt validated, and for M01
//! the whole attempt — one objective per block, one condition verdict per
//! block, one bound call per directive site, the binding every one of the 43
//! keys registered, and the four requirement rows the attempt produced. That
//! is a real production run over the owner's installation, not a paraphrase of
//! the acceptance assertions, and it carries no original bytes: keys, digests,
//! counts, operation codes and verdicts only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::survey_mission_control_programs;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::mission_control::{DirectiveDisposition, MeasuredControlRecord};
use cs_script::bindings::Lowering;

/// Every acceptance test the report must see pass: the prefixes are unique to
/// this task — stage `.03`'s adapter suite and the parent's own integration
/// suite (`M01-LC-DIRECTIVE-LOWERING`, #717) — so the report can claim the
/// selected assertions are exactly this task's tests.
const ACCEPTANCE_PREFIXES: [&str; 2] = [
    "accept_m01_lc_lowering_adapter_",
    "accept_m01_lc_directive_lowering_",
];

/// The findings documents this task's vocabulary and disposition chain rest
/// on, one entry per slug, so the artifact records which of them the candidate
/// tree actually holds.
const RECORDED_FINDINGS: [&str; 6] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-a-objective-directive-parser",
    "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
    "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives",
    "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
    "2026-10-06-m01-lc-directive-meaning",
];

/// The M01 row label the census publishes.
const M01: &str = "zbd/c1c/m01";

/// The numbers the review method reports, each derived from the census this
/// run produced (never written down by hand).
struct Observed {
    missions: usize,
    measured: usize,
    complete: usize,
    absent: usize,
    campaign_ready: bool,
    m01_blocks: u32,
    m01_sites: u32,
    m01_keys: usize,
    m01_measured: usize,
    m01_terminal: usize,
    m01_unmeasured: usize,
    m01_bindings: usize,
    m01_objectives: u32,
    m01_conditions: usize,
    m01_calls: usize,
    m01_validated: bool,
    unmet_rows: usize,
    own_tests: usize,
}

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced rather than written down.
fn review_method(observed: &Observed) -> String {
    let Observed {
        missions,
        measured,
        complete,
        absent,
        campaign_ready,
        m01_blocks,
        m01_sites,
        m01_keys,
        m01_measured,
        m01_terminal,
        m01_unmeasured,
        m01_bindings,
        m01_objectives,
        m01_conditions,
        m01_calls,
        m01_validated,
        unmet_rows,
        own_tests,
    } = *observed;
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of cs_app::mission_control::survey_mission_control_programs over the installation \
         (m01-directive-lowering.json). Claim is implemented only. MEASURED: M01's control record \
         now lowers into a RawProgram the adapter assembles from the record itself - {m01_blocks} \
         numbered block(s) became {m01_objectives} RawObjective(s) each carrying one lowered \
         condition ({m01_conditions} condition verdict(s)), {m01_sites} directive site(s) became \
         {m01_calls} RawCall(s) each bound through {m01_bindings} registry binding(s) built from \
         the record's own key dispositions, and MissionProgram::validate reports \
         {m01_validated}. The vocabulary partitions into {m01_measured} measured key(s) bound to \
         their DirectiveOperation and {m01_terminal} terminal outcome key(s) bound to \
         Lowering::Finish, with {m01_unmeasured} key(s) left unmeasured over the whole of M01's \
         {m01_keys} distinct keys - so the mission_identity, objective_identity, objective_condition \
         and call_arguments rows are all met from the attempt, is_complete() is true for \
         {M01}, complete_missions() reports {complete} of the {measured} measured row(s) over \
         {missions} mission-scoped reader(s), and campaign_ready() is {campaign_ready} while \
         {absent} reader(s) declare no control program at all and the corpus still spells keys no \
         finding covers ({unmet_rows} unmet lowering row(s) corpus-wide). A bound call emits \
         Action::Directive for the host: that is the emission of the measured operation, NOT \
         evidence that a world-side handler exists, so measured stays unimplemented and only the \
         two outcome spellings reach an engine operation. LIMITS OF WHAT WAS MEASURED: this is a \
         static lowering of the record into the IR - no original program was run and no mission \
         was played, so nothing here is verified_original behaviour, and the residual unknowns the \
         stage A-D findings recorded stay named on the rows and keys that carry them. \
         FAIL-CLOSED WITNESSES: an unmeasured key, a block the condition lowering refused, a \
         scalar beside its key, an unreadable block, an empty record and a record whose attempt \
         produced no program all still refuse by name with their rows unmet. TEST-SELECTION NOTE: \
         the prefixes accept_m01_lc_lowering_adapter_ and accept_m01_lc_directive_lowering_ are \
         unique to this task, so the {own_tests} discovered assertions are exactly this task's \
         tests. Validated with tools/validate_evidence.py --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_directive_lowering_writes_the_acceptance_report() {
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

    // The second production observation: the whole lowering attempt the rows
    // are derived from, plus the corpus gate it feeds.
    let census = survey_mission_control_programs(&game_dir).expect("the census surveys");
    let row = census.row(M01).expect("M01 is present");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();

    let m01_measured = record.measured().len();
    let m01_terminal = record.implemented().len();
    let m01_unmeasured = record.unmeasured().len();
    assert_eq!(
        m01_measured + m01_terminal + m01_unmeasured,
        record.keys().len(),
        "the three dispositions partition M01's vocabulary"
    );
    assert_eq!(
        (
            record.blocks(),
            record.sites(),
            record.vocabulary(),
            lowered.bindings()
        ),
        (58, 353, 43, 43),
        "the task's figures for M01: blocks, sites, keys and registry bindings"
    );
    assert!(
        attempt.conditions.iter().all(|outcome| matches!(
            outcome,
            cs_content::mission_control::ConditionOutcome::Lowered
        )),
        "every M01 block lowered: {:?}",
        attempt.conditions
    );
    assert!(
        attempt
            .calls
            .iter()
            .all(|outcome| matches!(outcome, cs_content::mission_control::CallOutcome::Bound)),
        "every M01 site bound: {:?}",
        attempt.calls
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "every M01 key registered: {:?}",
        attempt.unbound_keys
    );
    assert_eq!(
        attempt.validation.as_deref(),
        Some(&[][..]),
        "the lowered program validates: {:?}",
        attempt.validation
    );
    assert!(
        row.is_complete() && record.is_complete(attempt),
        "M01's row reports complete off its own attempt"
    );

    let complete = census.complete_missions();
    let unmet_rows: usize = census
        .measured_rows()
        .map(|row| {
            row.lowering()
                .expect("a measured row lowers")
                .unmet()
                .count()
        })
        .sum();
    assert!(
        !census.campaign_ready() && complete.contains(&M01),
        "M01 is complete and the campaign gate is not: {complete:?}"
    );

    let observation_path = evidence_dir.join("m01-directive-lowering.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_lowering_observation(&census, &install_sha256, &candidate_tree, unmet_rows)
        ),
    )
    .expect("write m01-directive-lowering.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-DIRECTIVE-LOWERING\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&Observed {
            missions: census.len(),
            measured: census.measured_len(),
            complete: complete.len(),
            absent: census.archives_without_control_program().len(),
            campaign_ready: census.campaign_ready(),
            m01_blocks: record.blocks(),
            m01_sites: record.sites(),
            m01_keys: record.keys().len(),
            m01_measured,
            m01_terminal,
            m01_unmeasured,
            m01_bindings: lowered.bindings(),
            m01_objectives: attempt.objectives,
            m01_conditions: attempt.conditions.len(),
            m01_calls: attempt.calls.len(),
            m01_validated: attempt.validation.as_deref() == Some(&[][..]),
            unmet_rows,
            own_tests: suite.discovered as usize,
        })),
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

/// The whole observation as JSON: the corpus gate, and M01's attempt with the
/// binding every key registered — so the report's claims are re-derived from
/// the installation rather than copied from this task's own assertions.
fn render_lowering_observation(
    census: &cs_app::mission_control::RetailControlCensus,
    install_sha256: &str,
    candidate_tree: &str,
    unmet_rows: usize,
) -> String {
    let row = census.row(M01).expect("M01 is present");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();
    let complete = census.complete_missions();
    let recorded_rows: Vec<String> = RECORDED_FINDINGS
        .iter()
        .map(|slug| {
            format!(
                "{{\"slug\": {}, \"held\": {}}}",
                jstr(slug),
                finding_held(slug)
            )
        })
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"missions\": {}, \"measured\": {}, \
         \"absent\": {}, \"complete_missions\": {}, \"campaign_ready\": {}, \"corpus\": \
         {{\"unmet_lowering_rows\": {}}}, \
         \"m01\": {{\"mission\": {}, \"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"measured\": {}, \"terminal\": {}, \"unmeasured\": {}, \"bindings\": {}, \"objectives\": \
         {}, \"conditions\": [{}], \"calls\": {{\"bound\": {}, \"refused\": {}}}, \"unbound_keys\": \
         {}, \"validation\": {}, \"keys\": [{}], \"lowering_rows\": [{}]}}, \
         \"recorded_findings\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        census.len(),
        census.measured_len(),
        census.archives_without_control_program().len(),
        string_array(&complete.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>()),
        census.campaign_ready(),
        unmet_rows,
        jstr(
            &attempt
                .mission
                .as_ref()
                .map_or_else(|reason| reason.clone(), |id| id.clone())
        ),
        record.blocks(),
        record.sites(),
        record.vocabulary(),
        record.measured().len(),
        record.implemented().len(),
        record.unmeasured().len(),
        lowered.bindings(),
        attempt.objectives,
        attempt
            .conditions
            .iter()
            .map(|outcome| jstr(&condition_label(outcome)))
            .collect::<Vec<_>>()
            .join(", "),
        attempt
            .calls
            .iter()
            .filter(|outcome| matches!(outcome, cs_content::mission_control::CallOutcome::Bound))
            .count(),
        attempt
            .calls
            .iter()
            .filter(|outcome| matches!(
                outcome,
                cs_content::mission_control::CallOutcome::Refused(_)
            ))
            .count(),
        string_array(&attempt.unbound_keys),
        match &attempt.validation {
            None => "null".to_owned(),
            Some(errors) => string_array(errors),
        },
        render_keys(record, lowered).join(", "),
        render_lowering_rows(row.lowering().expect("a measured row lowers")).join(", "),
        recorded_rows.join(", "),
    )
}

/// One condition verdict as a short label.
fn condition_label(outcome: &cs_content::mission_control::ConditionOutcome) -> String {
    match outcome {
        cs_content::mission_control::ConditionOutcome::Lowered => "lowered".to_owned(),
        cs_content::mission_control::ConditionOutcome::Refused(field)
        | cs_content::mission_control::ConditionOutcome::Unreadable(field) => {
            format!("refused: {field}")
        }
    }
}

/// M01's directive keys with the binding the adapter registered for each: the
/// disposition the record measured, and the `Lowering` the registry carries —
/// the two tables this task compares.
fn render_keys(
    record: &MeasuredControlRecord,
    lowered: &cs_app::control_lowering::LoweredControlRecord,
) -> Vec<String> {
    record
        .keys()
        .iter()
        .map(|key| {
            let disposition = match key.disposition() {
                DirectiveDisposition::TerminalOutcome { outcome } => format!(
                    "{{\"kind\": \"terminal_outcome\", \"outcome\": {}}}",
                    jstr(outcome.label())
                ),
                DirectiveDisposition::Measured(directive) => format!(
                    "{{\"kind\": \"measured\", \"operation\": {}}}",
                    jstr(directive.operation.code())
                ),
                DirectiveDisposition::Unmeasured { reason } => format!(
                    "{{\"kind\": \"unmeasured\", \"reason\": {}}}",
                    jstr(reason.code())
                ),
            };
            let binding = lowered
                .registry()
                .get(&key.key)
                .map(|spec| match spec.lowering {
                    Lowering::Directive(operation) => format!(
                        "{{\"lowering\": \"directive\", \"operation\": {}}}",
                        jstr(operation.code())
                    ),
                    Lowering::Finish(outcome) => format!(
                        "{{\"lowering\": \"finish\", \"outcome\": {}}}",
                        jstr(&format!("{outcome:?}"))
                    ),
                    other => format!("{{\"lowering\": {}}}", jstr(&format!("{other:?}"))),
                });
            format!(
                "{{\"key\": {}, \"sites\": {}, \"disposition\": {}, \"binding\": {}}}",
                jstr(&key.key),
                key.sites,
                disposition,
                binding.unwrap_or_else(|| "null".to_owned()),
            )
        })
        .collect()
}

/// M01's lowering rows: requirement, verdict and the fields it still names.
fn render_lowering_rows(lowering: cs_content::mission_control::ControlLowering) -> Vec<String> {
    lowering
        .requirements()
        .iter()
        .map(|row| {
            format!(
                "{{\"requirement\": {}, \"met\": {}, \"measurement\": {}, \"unmeasured_fields\": \
                 {}}}",
                jstr(row.kind.code()),
                row.met,
                jstr(&row.measurement),
                string_array(&row.unmeasured_fields),
            )
        })
        .collect()
}

/// Whether `docs/findings/<slug>.md` exists in the candidate tree and is not
/// empty — the repository half of the citation chain.
fn finding_held(slug: &str) -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is where the crate lives");
    fs::read_to_string(root.join("docs/findings").join(format!("{slug}.md")))
        .is_ok_and(|text| !text.trim().is_empty())
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_directive_lowering.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-DIRECTIVE-LOWERING` written
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

/// Extracts the per-test results of the `accept_m01_lc_lowering_adapter_`
/// tests from a recorded `cargo test` output.
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

/// Whether a libtest name is one of this task's tests.
///
/// The prefix is matched on the name's **last** path segment, because libtest
/// prints an in-module unit test under its module path. Matching the whole
/// name instead would silently drop every in-module acceptance test from
/// `discovered`, from the assertion list and from the log.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::").next().is_some_and(|segment| {
        ACCEPTANCE_PREFIXES
            .iter()
            .any(|prefix| segment.starts_with(prefix))
    })
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
                 \"m01-directive-lowering.json\"]}}",
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

/// A JSON array of strings (an alias of [`str_array`] at the call sites that
/// read as data rather than as a command line).
fn string_array(items: &[String]) -> String {
    str_array(items)
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
