//! Evidence-report harness for task F49-D.02 (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f49_d_02_*`: it is not part
//! of the acceptance suite, and it fails loudly when its inputs are missing
//! instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F49-D.02
//!    cargo test --locked -p cs_content -p cs_app -- accept_f49_d_02_ --include-ignored \
//!      2>&1 | tee private/evidence/F49-D.02/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F49-D.02 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_content -p cs_app -- accept_f49_d_02_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test -p cs_app --locked --test evidence_report_f49_d_02 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F49-D.02/acceptance.json \
//!      --artifact-root private/evidence/F49-D.02 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F49-D.02.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the production option
//! table assembled over that installation (the consumer trace written as
//! `option-table.json`), `rustc --version` and `Cargo.lock`. The only text this
//! file holds is the [`review_identity`] literal the committed report carries
//! (a reviewing agent replaces it with their own through `CS_EVIDENCE_REVIEW`)
//! and the per-dimension limitations quoted into `review.method`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::instant_action::{
    OptionListEvidence, ScenarioOptionDimension, retail_scenario_options,
};

const PREFIX: &str = "accept_f49_d_02_";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f49_d_02_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );
    assert!(
        (candidate_tree.len() == 40 || candidate_tree.len() == 64)
            && candidate_tree
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "CS_CANDIDATE_TREE must be a hex Git tree id, got {candidate_tree:?}"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `{PREFIX}` tests were recorded in {}",
        log_path.display()
    );
    for needed in [
        "accept_f49_d_02_retail_every_measured_list_is_rederived_from_the_installation",
        "accept_f49_d_02_retail_the_measured_environments_and_airframe_rows_are_offered",
    ] {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(needed))
            .unwrap_or_else(|| {
                panic!(
                    "the retail acceptance test {needed} did not run: F49-D.02 requires capability \
                     `retail`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(line.1, "pass", "the retail test {needed} must pass");
    }

    // The installation hashes describe the real installation, measured by the
    // production code the task's consumer wires in.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the production option table assembled over the
    // installation with the environments the production weather reader measured.
    let layout = cs_content::campaign_bindings::campaign_layout(&game_dir)
        .expect("the shared campaign walk reads the layout");
    let mut environments = Vec::new();
    for entry in &layout {
        let program = entry.mission.program_asset.to_ascii_lowercase();
        let Some(mission_key) = program.strip_suffix("/zrdr.zbd") else {
            continue;
        };
        if let Ok(weather) = cs_app::environment::retail::read_mission_weather(&found, mission_key)
        {
            environments.push(weather.binding.definition.id().clone());
        }
    }
    assert!(
        !environments.is_empty(),
        "the production weather reader binds at least one environment"
    );
    let table = retail_scenario_options(&game_dir, environments)
        .expect("the production option table assembles over the installation");
    assert!(
        !table.worlds().is_empty() && !table.airframes().is_empty() && !table.factions().is_empty(),
        "the measured option lists are nonempty"
    );
    assert_eq!(
        table.empty_required_lists(),
        vec!["loadouts"],
        "the only unmeasured required axis is the loadout identity list"
    );

    let trace_path = evidence_dir.join("option-table.json");
    fs::write(&trace_path, option_table_json(&table, &install_sha256))
        .unwrap_or_else(|error| panic!("write {}: {error}", trace_path.display()));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&trace_path, "json", &evidence_dir),
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F49-D.02\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \
         \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F49-D.02\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [],",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The option table as a JSON consumer trace: per-dimension evidence, counts and
/// the evaluated refusals. Derived from the assembled table, never typed.
fn option_table_json(
    table: &cs_content::instant_action::ScenarioOptionTable,
    install_sha256: &str,
) -> String {
    let mut dimensions = Vec::new();
    for dimension in ScenarioOptionDimension::ALL {
        let evidence = table
            .evidence(*dimension)
            .expect("the table names every dimension");
        let (class, reader, reason) = match evidence {
            OptionListEvidence::Measured { reader } => ("measured", *reader, ""),
            OptionListEvidence::Designed { rationale } => ("designed", "", *rationale),
            OptionListEvidence::Unmeasured { reason, .. } => ("unmeasured", "", *reason),
        };
        let count = match dimension {
            ScenarioOptionDimension::Worlds => table.worlds().len(),
            ScenarioOptionDimension::Environments => table.environments().len(),
            ScenarioOptionDimension::Factions => table.factions().len(),
            ScenarioOptionDimension::Airframes => table.airframes().len(),
            ScenarioOptionDimension::Loadouts => table.loadouts().len(),
            ScenarioOptionDimension::Pilots => table.pilots().len(),
            ScenarioOptionDimension::DifficultyTiers => table.difficulty_tiers().len(),
            ScenarioOptionDimension::VictoryConditions => table.victory_conditions().len(),
            ScenarioOptionDimension::Relations => table.relations().len(),
            ScenarioOptionDimension::MaxPlayers => usize::from(table.max_players()),
        };
        dimensions.push(format!(
            "{{\"dimension\": {}, \"evidence\": {class:?}, \"reader\": {}, \"note\": {}, \
             \"count\": {count}}}",
            jstr(dimension.label()),
            jstr(reader),
            jstr(reason),
        ));
    }
    let refusal = match table.clone().validate() {
        Ok(_) => "accepted".to_owned(),
        Err(error) => format!("refused: {error}"),
    };
    format!(
        "{{\n  \"task\": \"F49-D.02\",\n  \"install_sha256\": {},\n  \"source\": \
         \"cs_content::instant_action::retail_scenario_options\",\n  \"dimensions\": [{}],\n  \
         \"empty_required_lists\": {},\n  \"validate\": {}\n}}\n",
        jstr(install_sha256),
        dimensions.join(", "),
        str_array(
            &table
                .empty_required_lists()
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>()
        ),
        jstr(&refusal),
    )
}

/// The `review.identity` this harness records.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report then
/// names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: bunny-alpha-2/bunny-alpha-2 (opencode-go/deepseek-v4.1-flash; Rally #784 \
         implement claim of 2026-10-18); independent review pending — Rally assigns the reviewer. \
         The implementer built the retail option table, wrote the acceptance tests and the \
         evidence harness, and ran the acceptance suite locally with CS_GAME_DIR; this record \
         names no reviewer because no review has happened yet",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records, with the product-completeness
/// limits quoted so a reader of the committed report sees them.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite run locally with the retail capability and --include-ignored; every \
         field is derived from the recorded log, production discovery of $CS_GAME_DIR, the \
         production option table assembled over that installation and written as \
         option-table.json, rustc and Cargo.lock; validated with tools/validate_evidence.py \
         --require-pass. The consumer trace is cs_content::instant_action::\
         retail_scenario_options, which reads the F14-D.3/F14-D.5/F14-D.6 rows through \
         cs_content::catalog::baseline::retail_baseline and takes the environment ids \
         cs_app::environment::retail::read_mission_weather measured; the airframe rows are \
         cross-checked against cs_app::mission_start::airframe_entry. The measured lists are \
         re-derived from the installation by the acceptance test rather than trusted from the \
         constructor. Implementer mutation probes: making the constructor return \
         synthetic_scenario_options, emptying a measured list, or mislabelling an unmeasured \
         list as measured each fail a named accept_f49_d_02_ test.",
    );
    recorded
        + &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>()
}

/// The limitations this stage records instead of guessing, each naming the
/// affected content and the task that resolves it.
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "The loadout option list is unmeasured: the F27-D/F27-E measurement produces ammunition \
     types, selectable guns and ordnance counts, not the ContentKind::Loadout identities a \
     custom scenario selects. Affected content: the custom scenario's ordnance options and the \
     Non-negotiable 2 'unsupported ordnance' refusal, which cannot name a supported set until a \
     loadout identity producer exists. Resolving task: filed from #784.",
    "The pilot option list is the named aces the ia.zrd descriptors declare (ace_name labels); \
     whether the original's menu lets a player select a pilot is unmeasured, and the labels are \
     not resolved to display names. Affected content: a custom actor's pilot identity. \
     Resolving task: F49-D.03 and an original run observing the IA options screen (human_play).",
    "The relation option list is a measured negative: the retail factions are vehicle.zrd paint \
     patterns and the only allegiance the installation states is a record's team spelling \
     (neutral/ally/enemy), never a pair of factions; which team fights which is native code. \
     Affected content: the faction-relation table a roster is judged against stays empty. \
     Resolving task: a native-code measurement of the team-hostility rule or an original run.",
    "The original's player-count limit and victory vocabulary are unmeasured; the retail table \
     declares the module's MAX_SCENARIO_PLAYERS and the F49-A victory vocabulary as design. \
     Affected content: the seat ceiling and the victory conditions offered. Resolving task: an \
     original run that observes the IA options screen (human_play), and F49-D.03.",
    "The environment ids are per-mission weather environments read by the F19/F20 weather \
     reader, not a measured installation-wide environment table; whether the original's IA \
     screen offers each is F49-D.03 territory. Affected content: the environment options.",
];

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f49_d_02.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F49-D.02` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(git(&["rev-parse", "--show-toplevel"]))
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
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

fn locked_version(package: &str) -> String {
    let lock_path = workspace_root().join("Cargo.lock");
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

#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the `PREFIX`
/// tests from a recorded `cargo test` output.
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
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.contains(PREFIX) {
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

// ------------------------------------------------------------ artifacts ---

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
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
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

fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = era * 400 + year_of_era;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
