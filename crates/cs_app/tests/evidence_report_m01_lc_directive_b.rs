//! Evidence-report harness for task `M01-LC-DIRECTIVE-B` (#680):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_directive_b_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_directive_b_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-LC-DIRECTIVE-B/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVE-B \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_directive_b_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_directive_b -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-DIRECTIVE-B/acceptance.json
//!    --artifact-root private/evidence/M01-LC-DIRECTIVE-B --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-DIRECTIVE-B.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_mission_control_programs`] and records M01's measured control
//! record — the directive-key census whose spellings the findings document's
//! semantics apply to — as JSON. That is a real production run over the
//! owner's installation, not a paraphrase of the acceptance assertions, and
//! it carries no original bytes: ids, digests, byte extents, shapes and
//! dispositions only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::survey_mission_control_programs;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::mission_control::MeasuredControlRecord;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_directive_b_";

/// The measured key vocabulary this task's findings document measures
/// semantics for — M01's complete census, asserted by `accept_m01_lc_directive_a_`
/// and re-asserted here so a report regenerated on a changed installation can
/// never be written.
const M01_KEY_VOCABULARY: usize = 43;

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced rather than written down.
fn review_method(key_count: usize, site_count: u32, block_count: u32, own_tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of cs_app::mission_control::survey_mission_control_programs over the installation \
         (directive-census.json). Claim is implemented only. MEASURED: the findings document \
         docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md \
         records the runtime handler effects of M01's {key_count} spelled directive keys in \
         $CS_ENGINE_IMAGE (sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75) across {site_count} \
         sites in {block_count} numbered objective blocks - the acceptance suite asserts the \
         spellings those semantics apply to and the exact set of argument shapes whose list nests \
         a list, against the production census, so a drift between the document and the \
         data fails the run. The native-side evidence is STATIC CODE EVIDENCE ONLY: the \
         objective record layout, the CZMission::Update two-pass structure, the lifecycle state \
         machine (+0x5c8 dormant/awake/nap/done), the condition evaluators, the completion \
         pipeline and the target-flag helpers were read out of the executable with r2/objdump \
         and no original program was run, so nothing here is verified_original runtime \
         behaviour. LIMITS OF WHAT WAS MEASURED, each recorded in the findings document's \
         Unknowns section: (1) IDENTITY's third argument is never read by the measured parse \
         and no consumer was found; (2) the producers of the +0x24 bit-4 in-play flag and the \
         +0x91d despawn byte are world code outside the bound - consumers measured, producers \
         not; (3) the internals of the world-side consumers the pipeline calls (stoppoint, AI \
         net/team, turret and enemy wake calls) are measured at their call sites only; (4) \
         several parsers read payloads without tag checks, so malformed argument types are a \
         latent hazard observed but not exercised; (5) +0x570 (danger-zone field) and the \
         +0x71c128 pass-2 start rotation's writer are not traced. `unknowns` is empty because \
         the report lists no machine-readable unresolved issue beyond these documented bounds. \
         TEST-SELECTION NOTE: the prefix accept_m01_lc_directive_b_ is unique to this task, so \
         the {own_tests} discovered assertions are exactly this task's tests. Validated with \
         tools/validate_evidence.py --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_directive_b_writes_the_acceptance_report() {
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

    // The second production observation: M01's measured control record — the
    // exact key census whose spellings the findings document's semantics
    // apply to, with every site's argument shapes and dispositions.
    let census = survey_mission_control_programs(&game_dir).expect("the census surveys");
    let row = census.row("zbd/c1c/m01").expect("M01 is present");
    let record = row.record().expect("M01 declares a control program");
    assert_eq!(
        record.keys().len(),
        M01_KEY_VOCABULARY,
        "the census no longer reports the vocabulary the document measures"
    );
    let census_path = evidence_dir.join("directive-census.json");
    fs::write(
        &census_path,
        format!(
            "{}\n",
            render_directive_census(record, row.is_complete(), &install_sha256, &candidate_tree)
        ),
    )
    .expect("write directive-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-DIRECTIVE-B\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
            record.keys().len(),
            record.sites(),
            record.blocks(),
            suite.discovered as usize,
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

/// M01's measured control record as JSON: every directive key with its
/// measured argument shapes, sites and disposition, the record-level fields,
/// the block refusals and the lowering accounting. No original bytes.
fn render_directive_census(
    record: &MeasuredControlRecord,
    complete: bool,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
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
                    format!(
                        "{{\"kind\": \"terminal_outcome\", \"outcome\": {}}}",
                        jstr(outcome.label())
                    )
                }
                cs_content::mission_control::DirectiveDisposition::Measured(directive) => {
                    let evidence: Vec<String> =
                        directive.evidence.iter().map(|doc| jstr(doc)).collect();
                    let unknowns: Vec<String> =
                        directive.unknowns.iter().map(|u| jstr(u)).collect();
                    format!(
                        "{{\"kind\": \"measured\", \"operation\": {}, \"role\": {}, \
                         \"summary\": {}, \"evidence\": [{}], \"unknowns\": [{}]}}",
                        jstr(directive.operation.code()),
                        jstr(directive.operation.role().code()),
                        jstr(directive.summary),
                        evidence.join(", "),
                        unknowns.join(", "),
                    )
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
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"zbd/c1c/m01 \
         objectives.zrd\", \"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \"complete\": {}, \
         \"record_fields\": [{}], \"unclassified_record_keys\": [{}], \"block_refusals\": [{}], \
         \"keys\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        record.blocks(),
        record.sites(),
        record.vocabulary(),
        complete,
        fields.join(", "),
        unclassified.join(", "),
        refusals.join(", "),
        keys.join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_directive_b.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-DIRECTIVE-B` written relative
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

/// Extracts the per-test results of the `accept_m01_lc_directive_b_` tests
/// from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance selection.
/// A prefixed test that was skipped is recorded with the schema's `unknown`
/// status rather than counted as a pass, so a report can never claim an
/// assertion it did not run.
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
                 \"directive-census.json\"]}}",
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
