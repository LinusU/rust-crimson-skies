//! Evidence-report harness for task #574: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_t574_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. ```sh
//!    mkdir -p private/evidence/T574
//!    cargo test --workspace --locked -- accept_t574_ --include-ignored 2>&1 |
//!      tee private/evidence/T574/cargo-test.log
//!    ```
//!    (record the exit status of `cargo test`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T574 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t574_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> \
//!      cargo test --locked -p cs_content --test evidence_report_t574 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T574/acceptance.json \
//!      --artifact-root private/evidence/T574
//!    ```
//!    The `unknowns` array is deliberately non-empty — what the decoded keys
//!    mean to the original at runtime is unmeasured — so validate *without*
//!    `--require-pass`: that flag rejects reports with unresolved issues.
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T574.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs the production
//! census `cs_content::pilots::survey_retail_zeppelin_carrier` over the
//! installation and records each mission's archive spelling, SHA-256,
//! carrier presence, record count and decoded record spellings
//! (`zeppelin-carrier.json`) — a real production read, never a paraphrase of
//! the acceptance assertions.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::pilots::{neutral_traffic_support, survey_retail_zeppelin_carrier};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_t574_";

/// The two `#[ignore]`d retail measurements the task's acceptance rests on:
/// the census over the 50 carrying archives and the decoded-record
/// vocabulary check. A report whose log lacks either says nothing.
const RETAIL_TESTS: &[&str] = &[
    "accept_t574_retail_carrier_census_decodes_all_50_members",
    "accept_t574_retail_decoded_records_hold_the_measured_vocabulary",
];

/// The `review.identity` this harness records.
///
/// The literal below is the text this repository's committed
/// `docs/findings/evidence/T574.json` carries, and it is the first string in
/// this function on purpose: `tools/tests/test_evidence_review_identity.py`
/// reads a harness's recorded identity out of exactly this shape
/// (`fn review_identity() -> String`, a literal) and cross-checks it against
/// the committed report, so the text here and the report cannot drift apart
/// silently.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW`
/// and replaces the literal with it in the same commit, which is how the
/// report then names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: Devin SWE-2/swe2-max-1 (Rally #574 implement claim); reviewer: Devin \
         SWE-2/swe2-max-1 (Rally #574 review claim), a fresh session and context but the same \
         agent identity — not independent; the measured corpus was independently recounted \
         against the retail installation during review",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records: what was run, what it measured,
/// and the limits on the claim this report does not pretend away.
fn review_method(missions: usize, carriers: usize, records: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, \
         CS_CAPABILITIES includes retail); this harness derives every field from the recorded \
         log, production discovery of $CS_GAME_DIR, and a second production census through \
         cs_content::pilots::survey_retail_zeppelin_carrier (zeppelin-carrier.json): \
         {missions} mission directories, {carriers} carrying zeppelins.zrd, {records} decoded \
         records in all, every byte of every carrying member consumed by the production \
         decoder cs_formats::zbd::zeppelins::read_zeppelins_member. `candidate_tree` is the \
         tree of the commit the acceptance suite and this harness ran on; the only later delta \
         is this report's own copy under docs/findings/evidence/T574.json. MEASURED: the member \
         is one list holding one list of keyed records under the .zrd grammar (list word N \
         holds N - 1 children); a record is one placed zeppelin — a world-node binding, a \
         position/yaw/pitch pose, motion tuning, a net name, gasbag/engine/cannon bindings and \
         targets naming `player` or a sibling record's node. The link this task was filed to \
         find is a measured negative: no decoded record supplies a single input of the F33 \
         declared neutral-traffic schema (traffic index, pilot, airframe, faction, \
         survivability), so the roster lowering cannot consume the member without inventing \
         data — stated per field through cs_content::pilots::neutral_traffic_support, never \
         asserted as a boolean guess. LIMITS ON THE CLAIM (unmeasured original behaviour, none \
         claimed by this report): what the original does with any of the 26 keys is unmeasured \
         — the decoder reports KeyMeaning::Unknown for all of them and nothing here is \
         verified_original, no original run happened; whether a record is spawned as an actor, \
         and which records — if any — the original treats as neutral traffic, is likewise \
         unmeasured (the dependent task #772 is where a measured spawn consumption lands). The \
         three `unknowns` rows are the task's honest residue, so the report validates WITHOUT \
         --require-pass. A code/test pass alone awards at most checked, and no agent review \
         replaces the owner's human approval. Validated with tools/validate_evidence.py."
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t574_writes_the_acceptance_report() {
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
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested: a stale
    // report from another commit is exactly what this check refuses.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit"
    );

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log, ACCEPTANCE_PREFIX);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `{ACCEPTANCE_PREFIX}` tests were recorded in {}",
        log_path.display()
    );
    for retail in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| short_name(name) == *retail)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
        assert_eq!(status, "pass", "{retail} must pass");
    }

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the carrier census itself, re-run
    // through the production survey, recorded as spellings, counts and
    // digests — never member bytes.
    let census =
        survey_retail_zeppelin_carrier(&game_dir).expect("the production census reads retail");
    assert_eq!(census.install_sha256(), install_sha256);
    assert_eq!(census.content_sha256(), content_sha256);
    assert_eq!(census.missions().len(), 53);
    assert_eq!(census.missions_with_carrier().count(), 50);
    assert_eq!(census.record_count(), 58);
    assert!(!census.installation_scope_carrier());
    let omissions: Vec<String> = census
        .missions_without_carrier()
        .map(|row| format!("{}/{}", row.group(), row.mission()))
        .collect();
    assert_eq!(omissions, ["c1/m02", "c2/m01", "c5/mp2"]);
    for (_, record) in census.records() {
        assert!(
            !neutral_traffic_support(record).can_lower(),
            "record {} must not lower into declared neutral traffic",
            record.node()
        );
    }

    let mission_rows: Vec<String> = census
        .missions()
        .iter()
        .map(|row| {
            let nodes: Vec<String> = row
                .member()
                .map(|member| {
                    member
                        .records()
                        .iter()
                        .map(|record| jstr(record.node()))
                        .collect()
                })
                .unwrap_or_default();
            let teams: Vec<String> = row
                .member()
                .map(|member| {
                    member
                        .records()
                        .iter()
                        .filter_map(|record| record.team().map(jstr))
                        .collect()
                })
                .unwrap_or_default();
            format!(
                "{{\"archive\": {}, \"sha256\": {}, \"carrier_present\": {}, \
                 \"record_count\": {}, \"nodes\": [{}], \"teams\": [{}]}}",
                jstr(row.archive()),
                jstr(row.archive_sha256()),
                row.carrier_present(),
                row.record_count(),
                nodes.join(", "),
                teams.join(", "),
            )
        })
        .collect();
    let team_rows: Vec<String> = census
        .team_spellings()
        .iter()
        .map(|(spelling, count)| format!("[{}, {count}]", jstr(spelling)))
        .collect();
    let carrier_path = evidence_dir.join("zeppelin-carrier.json");
    fs::write(
        &carrier_path,
        format!(
            "{{\n \"task_id\": \"T574\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"content_sha256\": {},\n \"member\": \"zeppelins.zrd\",\n \
             \"installation_scope_carrier\": {},\n \"missions\": {},\n \
             \"missions_with_carrier\": {},\n \"missions_without_carrier\": [{}],\n \
             \"records\": {},\n \"team_spellings\": [{}],\n \"rows\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            jstr(&content_sha256),
            census.installation_scope_carrier(),
            census.missions().len(),
            census.missions_with_carrier().count(),
            omissions
                .iter()
                .map(|omission| jstr(omission))
                .collect::<Vec<_>>()
                .join(", "),
            census.record_count(),
            team_rows.join(", "),
            mission_rows.join(",\n  "),
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", carrier_path.display()));

    let unknowns = [
        "What a `zeppelins.zrd` record means to the original at runtime is not established: \
         the 26-key grammar and every value shape are measured over all 50 carrying members, \
         but whether a record is spawned as an actor, which records — if any — the original \
         treats as neutral traffic, and what `team`, `deactivated`, `num_healthy_required`, \
         the `healthy`/`gasbags`/`cannon_health` spellings and the tuning floats do is \
         unmeasured (the decoder reports KeyMeaning::Unknown for all 26 keys). Affected \
         content: all 58 decoded records. Resolving path: original-run or executable evidence; \
         the dependent task #772 is where a measured spawn consumption lands.",
        "How a record maps to the declared schemas is unmeasured: `node` is a world-node \
         binding, not an airframe catalog id; `team` is mission vocabulary (`ally`/`enemy` \
         measured), not a faction id; and the encoding carries no traffic index, no pilot and \
         no survival policy, so `neutral_traffic_support` reports `can_lower() == false` for \
         every measured record — the F33 neutral-traffic lowering cannot consume the member \
         without inventing data. Affected content: the link F33-D's census left open between \
         DeclaredNeutralTraffic and original data.",
        "Which world node each record's `node` spelling binds is not resolved here: the \
         spellings are mission-authored node names whose resolution into the mission's world \
         container is outside this task's owner paths. Affected content: every record's \
         world placement. Resolving path: the world-actor spawn work that consumes the \
         binding (#772 and the M01 launch chain).",
    ]
    .iter()
    .map(|unknown| jstr(unknown))
    .collect::<Vec<_>>()
    .join(", ");

    let artifacts = [
        artifact(&log_path, "log", &evidence_dir),
        artifact(&carrier_path, "json", &evidence_dir),
    ];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T574\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
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
        jstr(&review_identity()),
        jstr(&review_method(
            census.missions().len(),
            census.missions_with_carrier().count(),
            census.record_count(),
        )),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_t574.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T574` written relative to the
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

/// Extracts the per-test results of the `accept_t574_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this
/// task's acceptance selection. A prefixed test that was skipped is recorded
/// with the schema's `unknown` status rather than counted as a pass, so a
/// report can never claim an assertion it did not run.
fn parse_suite(log: &str, prefix: &str) -> Suite {
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
        // `test <name> ... <status>`, possibly several per interleaved line.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.contains(prefix) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    for (_, status) in &suite.assertions {
        match *status {
            "pass" => suite.passed += 1,
            "fail" => suite.failed += 1,
            _ => suite.ignored += 1,
        }
    }
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// The status word a finished test line carries, if it carries one.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

/// The test's name without its module path, for matching a required test.
fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

// ------------------------------------------------------------ artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the
/// sibling crate implements (the validator re-hashes it independently).
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

// ------------------------------------------------------------ rendering ---

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
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
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = (if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
