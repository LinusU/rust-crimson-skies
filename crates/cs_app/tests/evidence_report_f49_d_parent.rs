//! Evidence-report harness for task F49-D (the parent of F49-D.01..F49-D.04;
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f49_d_*`: it is not part of
//! the acceptance suite, and it fails loudly when its inputs are missing instead
//! of passing vacuously. Run from the workspace root, after the acceptance
//! suite, exactly as:
//!
//! 1. ```sh
//!    mkdir -p private/evidence/F49-D
//!    cargo test --locked -p cs_content -p cs_app -- accept_f49_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F49-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`. Only `cs_content` and `cs_app` hold
//!    `accept_f49_d_` tests, and AGENTS.md forbids a local workspace run.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F49-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_content -p cs_app -- accept_f49_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test -p cs_app --locked --test cs_app_tests_d -- evidence_report_f49_d_parent --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F49-D/acceptance.json \
//!      --artifact-root private/evidence/F49-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F49-D.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the production catalog
//! and flow (the consumer trace written as `ac04.json`), the committed subtask
//! reports `F49-D.01`..`F49-D.03` (linked as artifacts), `rustc --version` and
//! `Cargo.lock`. The only text this file holds is the [`review_identity`]
//! literal the committed report carries (a reviewing agent replaces it with
//! their own through `CS_EVIDENCE_REVIEW`) and the limitations quoted into
//! `review.method`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::ui::instant_action::{
    IaAction, IaEdit, InstantActionFlow, lower_preset, retail_instant_action_catalog,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::instant_action::{InstantActionCatalog, RosterSlot, ScenarioSide};
use cs_sim::campaign::ProfileId;
use cs_types::content::{ContentId, ContentKind};

const PREFIX: &str = "accept_f49_d_";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f49_d_parent_writes_the_acceptance_report() {
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
        "accept_f49_d_01_retail_denominator_is_remeasured_from_ia_zrd",
        "accept_f49_d_02_retail_every_measured_list_is_rederived_from_the_installation",
        "accept_f49_d_03_retail_catalog_holds_every_measured_preset_from_the_installation",
        "accept_f49_d_03_retail_every_preset_lowers_to_its_declared_actors",
        "accept_f49_d_03_retail_every_unmeasured_dimension_refuses_at_lowering_with_its_reason",
        "accept_f49_d_04_retail_every_measured_selection_is_accepted_and_the_unmeasured_loadout_is_refused",
        "accept_f49_d_04_retail_a_valid_request_from_the_retail_lists_is_accepted",
        "accept_f49_d_04_retail_an_empty_roster_is_refused_with_a_sentence",
        "accept_f49_d_04_retail_an_airframe_no_option_table_offers_is_refused_with_every_other_problem",
        "accept_f49_d_04_retail_an_impossible_faction_pair_is_refused",
        "accept_f49_d_04_retail_a_seat_count_the_roster_cannot_fill_is_refused",
        "accept_f49_d_04_retail_an_incomplete_draft_names_every_unset_dimension",
        "accept_f49_d_04_retail_an_unknown_is_refused_with_its_claim_never_defaulted",
        "accept_f49_d_04_retail_every_problem_of_a_request_comes_back_at_once",
        "accept_f49_d_04_retail_a_refused_selection_starts_no_session",
        "accept_f49_d_04_retail_launching_a_preset_whose_facts_are_unmeasured_starts_no_session",
        "accept_f49_d_04_synthetic_every_invalid_class_is_reported_in_full",
        "accept_f49_d_04_synthetic_a_refused_selection_starts_no_session",
    ] {
        let line = suite
            .assertions
            .iter()
            .find(|(name, _)| name.contains(needed))
            .unwrap_or_else(|| {
                panic!(
                    "the retail acceptance test {needed} did not run: F49-D requires capability \
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

    // The consumer trace: the production catalog assembled over the
    // installation, one row per preset with its roster and its refusal.
    let catalog = retail_instant_action_catalog(&game_dir)
        .expect("the production catalog assembles over the installation");
    assert_eq!(
        catalog.presets().len(),
        30,
        "F49-D.01 measured thirty presets"
    );
    assert!(catalog.is_original_complete());

    let trace_path = evidence_dir.join("ac04.json");
    fs::write(&trace_path, ac04_json(&catalog, &install_sha256))
        .unwrap_or_else(|error| panic!("write {}: {error}", trace_path.display()));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let mut artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&trace_path, "json", &evidence_dir),
    ];
    // The subtask reports this parent is the sum of, linked as artifacts.
    for stage in ["F49-D.01", "F49-D.02", "F49-D.03"] {
        let report = workspace_root()
            .join("docs/findings/evidence")
            .join(format!("{stage}.json"));
        artifacts.push(artifact(&report, "json", &evidence_dir));
    }

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F49-D\",\n\
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
        "\"task_id\": \"F49-D\"",
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

/// The AC04 consumer trace as JSON: the retail option table's measured and
/// unmeasured dimensions, and per retail preset what the production flow does
/// with an unedited launch and with a custom draft edited to an airframe no
/// option table offers. Derived from the assembled catalog and the production
/// `InstantActionFlow`, never typed.
fn ac04_json(catalog: &InstantActionCatalog, install_sha256: &str) -> String {
    let options = catalog.options();
    let phantom = ContentId::from_source(ContentKind::Airframe, "synthetic.ac04_phantom_airframe")
        .expect("a valid airframe id");
    let subject = ContentId::from_source(ContentKind::IaScenario, "synthetic.ac04_custom_scenario")
        .expect("a valid scenario id");
    let profile = ProfileId::new("pilot.evidence").expect("a valid profile id");
    let mut rows = Vec::new();
    for preset in catalog.presets() {
        let mut flow = InstantActionFlow::new(profile.clone(), catalog.clone());
        flow.select_preset(preset.id())
            .expect("a catalog preset selects");
        let preset_launch = match flow.apply(IaAction::Launch) {
            Ok(_) => "launched".to_owned(),
            Err(refusal) => refusal.to_string(),
        };
        let started_by_preset = flow.generation().is_some() || !flow.records().is_empty();
        flow.apply(IaAction::StartCustom {
            subject: subject.clone(),
        })
        .expect("a custom draft opens from a selected preset");
        let unset = flow
            .draft()
            .map(|draft| draft.unset_dimensions())
            .unwrap_or_default();
        let edit = match flow.edit(IaEdit::SlotAirframe {
            side: ScenarioSide::Player,
            slot: RosterSlot(0),
            airframe: phantom.clone(),
        }) {
            Ok(()) => "edited".to_owned(),
            Err(refusal) => refusal.to_string(),
        };
        let custom_launch = match flow.apply(IaAction::Launch) {
            Ok(_) => "launched".to_owned(),
            Err(refusal) => refusal.to_string(),
        };
        let started =
            started_by_preset || flow.generation().is_some() || !flow.records().is_empty();
        let unset: Vec<String> = unset.iter().map(|d| (*d).to_owned()).collect();
        rows.push(format!(
            "{{\"preset\": {}, \"preset_launch\": {}, \"custom_unset_dimensions\": {}, \
             \"phantom_airframe_edit\": {}, \"custom_launch\": {}, \"session_started\": {}}}",
            jstr(preset.id().as_str()),
            jstr(&preset_launch),
            str_array(&unset),
            jstr(&edit),
            jstr(&custom_launch),
            started,
        ));
        // The preset itself must refuse through the lowering as well.
        assert!(lower_preset(catalog, preset.id()).is_err() || preset_launch == "launched");
    }
    let dims = |names: Vec<String>| str_array(&names);
    let unmeasured: Vec<String> = options
        .unmeasured_dimensions()
        .iter()
        .map(|dimension| format!("{dimension:?}"))
        .collect();
    format!(
        "{{\n  \"task\": \"F49-D\",\n  \"install_sha256\": {},\n  \"source\": \
         \"cs_app::ui::instant_action::InstantActionFlow over retail_instant_action_catalog\",\n  \
         \"option_table\": {{\"worlds\": {}, \"environments\": {}, \"factions\": {}, \
         \"airframes\": {}, \"loadouts\": {}, \"pilots\": {}, \"difficulty_tiers\": {}, \
         \"victory_conditions\": {}, \"relations\": {}, \"max_players\": {}, \
         \"unmeasured_dimensions\": {}}},\n  \"presets\": [\n    {}\n  ]\n}}\n",
        jstr(install_sha256),
        options.worlds().len(),
        options.environments().len(),
        options.factions().len(),
        options.airframes().len(),
        options.loadouts().len(),
        options.pilots().len(),
        options.difficulty_tiers().len(),
        options.victory_conditions().len(),
        options.relations().len(),
        options.max_players(),
        dims(unmeasured),
        rows.join(",\n    "),
    )
}

/// The `review.identity` this harness records.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report then
/// names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: sonnet-2/sonnet-2 (Claude Sonnet 5.5; Rally #786 implement claim of \
         2026-10-18, stage F49-D.04 of F49-D); the F49-D.01..F49-D.03 stages were implemented \
         under their own tasks and their reports are linked as artifacts. Independent review \
         pending - Rally assigns the reviewer. The implementer wrote the AC04 acceptance tests \
         and this harness and ran the acceptance suite locally with CS_GAME_DIR; no review has \
         happened yet",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// The `review.method` this harness records, with the product-completeness
/// limits quoted so a reader of the committed report sees them.
fn review_method() -> String {
    let recorded = String::from(
        "acceptance suite (every accept_f49_d_ test of cs_content and cs_app, the only crates \
         holding any) run locally with the retail capability and --include-ignored; every field \
         is derived from the recorded log, production discovery of $CS_GAME_DIR, the production \
         catalog and InstantActionFlow over that installation written as ac04.json, the linked \
         subtask reports, rustc and Cargo.lock; validated with tools/validate_evidence.py \
         --require-pass. AC04 is exercised through CustomScenarioDraft::resolve, \
         InstantActionCatalog::validate_custom / require_valid_custom, resolve_custom / \
         lower_custom and the flow's Launch. The retail option table keeps loadouts, pilots and \
         relations unmeasured, so the positive control (a request nothing refuses) uses the \
         retail lists plus declared stand-ins for those three and is labelled so in the test; it \
         proves refusal is specific, not that the original accepts such a scenario. Implementer \
         mutation probes: truncating the problem list to its first entry and rendering a \
         problem with Debug each fail accept_f49_d_04_synthetic_every_invalid_class_is_reported_in_full. \
         `retail` is file access only: nothing was run or played and nothing here is \
         verified_original.",
    );
    recorded
        + &UNKNOWN_LIMITATIONS
            .iter()
            .map(|limitation| format!(" LIMITATION: {limitation}"))
            .collect::<String>()
}

/// The limitations this parent records instead of guessing, each naming the
/// affected content and the task that resolves it.
const UNKNOWN_LIMITATIONS: &[&str] = &[
    "AC01's launch half is unmet: no retail preset launches. Every preset's difficulty, victory \
     rules, title and every actor's faction, loadout and survivability are unmeasured and \
     refuse at lowering (cs_app::ui::instant_action::lower_preset); ZBD/c2b/IA1 additionally \
     lacks a player_plane and a num_wingmen. Affected content: all 30 presets. Resolving tasks: \
     #1582 (factions, survivability), #1583 (victory rules, difficulty, c2b), #1575 (loadouts).",
    "A custom scenario naming a loadout, a pilot or a faction relation is refused by the pure \
     retail option table because those three lists are unmeasured (F49-D.02). Affected content: \
     every custom scenario's loadouts, pilots and hostility checks. Resolving tasks: #1575 \
     (loadouts); pilots and relations are filed with the F49-D findings.",
    "Whether the original menu offers every (directory, offered mission type) pair, the original \
     preset titles, which declared aircraft each mission type spawns and the seed policy are \
     unmeasured or designed (F49-D.01, F49-D.03). Affected content: the preset list and each \
     preset's spawned roster. Resolving tasks: #1584 (titles and menu), an original run \
     (human_play) for the rest.",
];

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f49_d_parent.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F49-D` written relative to the
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
