//! Evidence-report harness for task `M01-LC-DIRECTIVE-C` (#681):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_directive_c_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_lc_directive_c_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-LC-DIRECTIVE-C/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVE-C \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_directive_c_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_directive_c -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-LC-DIRECTIVE-C/acceptance.json
//!    --artifact-root private/evidence/M01-LC-DIRECTIVE-C --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-DIRECTIVE-C.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**, taken through a different production path
//! than the census the acceptance suite uses: the harness re-reads the member
//! [`cs_content::mission_control::control_member`] selects, re-decodes it with the
//! production `.zrd` decoder and records every site of this stage's scoped
//! directive keys with its decoded arguments (`directive-sites.json`) — the data
//! the findings document's per-key sections quote. That is a real production
//! read of the owner's installation, not a paraphrase of the acceptance
//! assertions, and it carries no original bytes: key names, block names and
//! argument values only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_control::{read_control_member, survey_mission_control_programs};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_directive_c_";

/// The scoped vocabulary this task's findings document measures: the keys M01
/// spells whose handlers act on actors, the world or the animation system, plus
/// the siblings the same parser handles and M01 never spells. Asserted verbatim
/// by the `accept_m01_lc_directive_c_` tests, so a report regenerated on an
/// installation whose census disagrees can never be written.
const SCOPED_KEYS_M01_SPELLS: &[&str] = &[
    "ANIM_STATE",
    "SET_AI_NET",
    "TRAVELERS",
    "WAKEUP_ENEMIES",
    "WAKEUP_GENERATOR",
    "WAKEUP_ZEP_TURRETS",
    "WAKE_ANIM",
];

const SCOPED_KEYS_M01_DOES_NOT_SPELL: &[&str] = &[
    "SET_AI_ATTACK_RADIUS",
    "SET_AI_TEAM",
    "SLEEP_ANIM",
    "START_TAXI",
    "WAKEUP_TURRETS",
    "WARP_VEHICLE",
];

/// How this run was reviewed, with every measured number **derived** from the
/// two production observations this same run produced rather than written down.
fn review_method(
    spelled: usize,
    documented_sites: usize,
    key_count: usize,
    block_count: u32,
    own_tests: usize,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and two production reads of \
         the installation: the control-program census and the decoded control member. Claim is \
         implemented only. MEASURED: the findings document \
         docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md \
         records what M01's AI, world and animation directives do, read out of the native handlers \
         in $CS_GAME_DIR/crimson.decrypted.exe (sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75): the wake handler at \
         0x469af0, the completion pass at 0x46a630, the objective transition at 0x46b160 and the \
         per-key handlers 0x4a97b0, 0x4bef70, 0x451720, 0x493fb0, 0x4940d0, 0x469e20, 0x46a000, \
         0x469f70, 0x46a2b0, 0x465b40, 0x4edda0 and the ANIM_STATE parse 0x4691d0 with its \
         evaluator 0x4697a0. The production census reports {spelled} of this stage's scoped keys \
         among M01's {key_count} distinct directive keys across {block_count} numbered objective \
         blocks, and the decoded member carries {documented_sites} sites of them; the acceptance \
         suite asserts the scoped vocabulary, the per-key argument shapes and every shipped site \
         with its decoded arguments, so a drift between the document and the data fails the run. \
         The native-side evidence is STATIC CODE EVIDENCE ONLY: addresses, guards, argument arity \
         and tag checks, the world object fields written and the update-time consumers were read \
         out of the executable with r2 and objdump and no original program was run, so nothing \
         here is verified_original runtime behaviour, and no disposition is implemented by this \
         task. LIMITS OF WHAT WAS MEASURED, each recorded in the document's Unknowns section: \
         (1) vehicle+0xd4's original name is not recoverable - START_TAXI clears it, the spawn \
         path sets it and it gates the AI think at 0x48a129; (2) SET_AI_ATTACK_RADIUS writes \
         three fields (r squared, -r, r) and which AI-node field each one is stays unresolved; \
         (3) WARP_VEHICLE's per-axis velocity factors and world+0x1e8 are measured as an \
         expression, not named; (4) the WAKEUP_TURRETS wildcard is measured as 'a * consumes \
         exactly one character and that character must be a digit' and no mission constrains its \
         intent; (5) TRAVELERS' polarity: the record field +0x59c, which is set only when child 1 \
         spells APPROACHING, selects the inside of the radius in both evaluator modes and its \
         absence selects the outside, and the parser reads no token naming that other pole; \
         (6) whether M01's TRAVELERS spelling takes the subject or the count mode \
         depends on a runtime node flag bit that the executable does not fix; (7) the anim state \
         enum is named up to 6 (UNDEFINED, DORMANT, RUNNING, EXECUTED, INVALID, CORRUPT, \
         INVALID_AND_RUNNING) from the engine's own debug table, and states outside it were not \
         explored. `unknowns` is empty because every scoped key reached a measured handler: no \
         scoped key is unresolved, and the items above are limits on what the measurements can \
         name rather than keys left unmeasured. TEST-SELECTION NOTE: the prefix \
         accept_m01_lc_directive_c_ is unique to this task, so the {own_tests} discovered \
         assertions are exactly this task's tests. Validated with tools/validate_evidence.py \
         --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_directive_c_writes_the_acceptance_report() {
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

    // The first production observation: the census's scoped keys.
    let census = survey_mission_control_programs(&game_dir).expect("the census surveys");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    let spelled: Vec<&cs_content::mission_control::MeasuredDirectiveKey> = record
        .keys()
        .iter()
        .filter(|key| SCOPED_KEYS_M01_SPELLS.contains(&key.key.as_str()))
        .collect();
    let spelled_names: Vec<&str> = spelled.iter().map(|key| key.key.as_str()).collect();
    assert_eq!(
        spelled_names, SCOPED_KEYS_M01_SPELLS,
        "the census no longer reports the scoped vocabulary the document maps"
    );

    // The second production observation, through the control-member rule and the
    // production `.zrd` decoder rather than the census: every scoped site with
    // its decoded arguments.
    let (document, row) =
        read_control_member(&game_dir, "zbd/c1c/m01").expect("M01's control member decodes");
    let sites = scoped_sites(&document);
    assert_eq!(
        sites.len(),
        spelled.iter().map(|key| key.sites as usize).sum::<usize>(),
        "the decoded member carries a different number of scoped sites than the census reports"
    );
    let sites_path = evidence_dir.join("directive-sites.json");
    fs::write(
        &sites_path,
        format!(
            "{}\n",
            render_sites(&sites, row.name.as_str(), &install_sha256, &candidate_tree)
        ),
    )
    .expect("write directive-sites.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&sites_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-LC-DIRECTIVE-C\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
            spelled.len(),
            sites.len(),
            record.keys().len(),
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

/// One scoped directive site: which numbered objective spells it, and the
/// decoded value beside the key.
struct ScopedSite {
    block: String,
    key: String,
    arguments: String,
}

/// Every site of a scoped key in the decoded control member, in document order.
fn scoped_sites(document: &ZrdValue) -> Vec<ScopedSite> {
    let mut sites = Vec::new();
    for (block, value) in zrd_flat_fields(objective_record(document)) {
        if !block.starts_with("OBJECTIVE") {
            continue;
        }
        for (key, arguments) in zrd_flat_fields(value) {
            if SCOPED_KEYS_M01_SPELLS.contains(&key) {
                sites.push(ScopedSite {
                    block: block.to_owned(),
                    key: key.to_owned(),
                    arguments: render_value(arguments),
                });
            }
        }
    }
    sites
}

/// A total, lossless rendering of the four `.zrd` node kinds: a rounded float
/// would let a decoding change hide behind it, and a nested list keeps its
/// brackets because two scoped keys spell lists the mission IR cannot carry.
fn render_value(value: &ZrdValue) -> String {
    match value {
        ZrdValue::Int(v) => format!("int {v}"),
        ZrdValue::Float(v) => format!("float {v:?}"),
        ZrdValue::Text(v) => format!("text {v}"),
        ZrdValue::List(children) => {
            let inner: Vec<String> = children.iter().map(render_value).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

/// The scoped sites as JSON: the data the findings document's per-key sections
/// quote. No original bytes — member name, block names, key names and argument
/// values only.
fn render_sites(
    sites: &[ScopedSite],
    member: &str,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let rows: Vec<String> = sites
        .iter()
        .map(|site| {
            format!(
                "{{\"block\": {}, \"key\": {}, \"arguments\": {}}}",
                jstr(&site.block),
                jstr(&site.key),
                jstr(&site.arguments)
            )
        })
        .collect();
    let spelled = SCOPED_KEYS_M01_SPELLS
        .iter()
        .map(|key| jstr(key))
        .collect::<Vec<String>>()
        .join(", ");
    let absent = SCOPED_KEYS_M01_DOES_NOT_SPELL
        .iter()
        .map(|key| jstr(key))
        .collect::<Vec<String>>()
        .join(", ");
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"zbd/c1c/m01 {}\", \
         \"scoped_keys_m01_spells\": [{}], \"scoped_keys_m01_does_not_spell\": [{}], \"sites\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        member,
        spelled,
        absent,
        rows.join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_directive_c.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-DIRECTIVE-C` written relative
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

/// Extracts the per-test results of the `accept_m01_lc_directive_c_` tests from
/// a recorded `cargo test` output.
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
/// prints an in-module unit test under its module path. Matching the whole name
/// instead would silently drop every in-module acceptance test from
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
                 \"directive-sites.json\"]}}",
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
