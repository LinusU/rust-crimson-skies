//! Evidence-report harness for task `M01-PLAY-TRIGGERS-PARTS` (#1481):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_triggers_parts_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_play_triggers_parts_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-PARTS/cargo-test.log
//!    ```
//!    (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-PARTS \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_parts_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_parts -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-TRIGGERS-PARTS/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-TRIGGERS-PARTS`
//!    (exit 0, `structurally_valid`). `--require-pass` is deliberately not used:
//!    it refuses any report whose `unknowns` array is non-empty
//!    (`tools/validate_evidence.py`), and this report carries the unresolved
//!    issues the owner's 2026-09-28 directive forbids deleting — the missing
//!    measured engine integrity, the untraced in-play-bit producers and the
//!    actor-level `INACTIVE` ladders. The pass/fail facts are carried by
//!    `command.exit_code` and the `tests` counts, which the plain check does
//!    evaluate.
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-TRIGGERS-PARTS.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness rebuilds M01's composition
//! headlessly and records, per spelled part-state chain, the live entity the
//! spawn host bound, the damage node its owner's graph carries and the state
//! the session's damage authority reports — beside the record's own `engines`
//! lists and the `INACTIVE` ladder census read from the lowered program. Names,
//! numbers and digests only, never original bytes.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPartReport, MissionStage, build_headless, stage_for, teardown,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_facts::WorldOperands;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_script::ir::Condition;
use cs_sim::damage::PartState;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_parts_";

/// The measured shape this task's acceptance asserts — asserted verbatim by the
/// `accept_m01_play_triggers_parts_` tests, so a report regenerated on an
/// installation whose records disagree can never be written.
const M01_OPERAND_CHAINS: usize = 42;
const M01_PART_CHAINS: usize = 39;
const M01_WORKERS_PART_CHAINS: usize = 27;
const M01_PIRATE_PART_CHAINS: usize = 12;

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_parts_writes_the_acceptance_report() {
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

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let observation = observe_parts(&game_dir);
    let observation_path = evidence_dir.join("part-states.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_parts(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write part-states.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-TRIGGERS-PARTS\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": {}, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        observation.ticks_start,
        observation.ticks_end,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        unknown_array(&observation.unknowns),
        jstr(&reviewer),
        jstr(&review_method(&observation, suite.discovered as usize)),
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

/// How this run was reviewed, with every measured number **derived** from the
/// production reads this same run produced rather than written down.
fn review_method(observation: &PartRun, own_tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         (part-states.json): cs_app::mission_session::stage_for rebuilt M01's stage, \
         build_headless composed it, the spawn host bound one live part-state entity per spelled \
         chain and MissionHost::step folded the observer's rows, so the artifact records what the \
         composition really holds — {spelled} spelled member chains of which {bound} are \
         part-state chains with a live entity and a damage node, {workers} under workersvoyagezep \
         and {pirates} under piratezep, against the {engines} engine node names M01's own \
         zeppelins.zrd records state. Claim is implemented only. MEASURED: the chains, the \
         container resolution, the record's engines lists, the ladder thresholds and the in-play \
         polarity all come from the installation through the production readers. DESIGNED: the \
         part damage nodes' integrity pool (one labelled unit, because the installation states no \
         engine pool anywhere), and the composition's own choice of one ECS entity per part-state \
         chain — this reimplementation's contract, never an original measurement. Nothing here is \
         verified_original and no original executable ran. TEST-SELECTION NOTE: the prefix \
         {ACCEPTANCE_PREFIX} is unique to this task, so the {own_tests} discovered assertions are \
         exactly this task's tests. candidate_tree is the tree of the commit the acceptance suite \
         and this harness both ran on; the only later delta on the branch is this report's own \
         copy under docs/findings/evidence/ plus the finding's prose. Validated with \
         tools/validate_evidence.py (structure, counts and artifact hashes; --require-pass is not \
         used because the report's unknowns must not be deleted).",
        spelled = observation.chains,
        bound = observation.bound.len(),
        workers = observation.workers,
        pirates = observation.pirates,
        engines = observation.record_engines,
    )
}

/// What the second production observation records.
struct PartRun {
    ticks_start: u64,
    ticks_end: u64,
    /// Every member chain the lowered program spells.
    chains: usize,
    /// `workersvoyagezep` parts bound.
    workers: usize,
    /// `piratezep` parts bound.
    pirates: usize,
    /// The engine node names M01's `zeppelins.zrd` records state, in total.
    record_engines: usize,
    /// The bound parts: chain, actor, node, the authority's state.
    bound: Vec<BoundRow>,
    /// The `INACTIVE` ladders the program carries: `(block, threshold, members)`.
    ladders: Vec<(usize, u32, usize)>,
    /// The bound but *actor-level* operands (the one-element chains).
    actor_level: Vec<String>,
    /// The absences the report names instead of filling in.
    unknowns: Vec<String>,
}

/// One bound part as the second observation records it.
struct BoundRow {
    chain: Vec<String>,
    actor: String,
    node: String,
    state: String,
    in_play: Option<bool>,
    position: [f64; 3],
}

/// Rebuilds M01's composition and records what the spawn host bound and what
/// the damage authority says about each part — a real production run over the
/// owner's installation, not a paraphrase of the acceptance assertions.
fn observe_parts(game_dir: &Path) -> PartRun {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M06").expect("the label is valid"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let stage: MissionStage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    let operands = WorldOperands::of(&stage.host.control);
    let ladders = ladder_census(&stage.host.control);

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    for _ in 0..4 {
        app.update();
    }
    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the spawn host installed the parts report")
        .clone();
    let resolver = app
        .world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .clone();
    let rows: Vec<BoundRow> = report
        .parts
        .iter()
        .map(|part| {
            let position = app
                .world()
                .get::<bevy::prelude::Transform>(part.entity)
                .map(|transform| {
                    let [x, y, z] = transform.translation.to_array();
                    [f64::from(x), f64::from(y), f64::from(z)]
                })
                .unwrap_or([0.0; 3]);
            let state = resolver
                .part_state(&part.actor, &part.node)
                .map_or("unregistered", PartState::label);
            BoundRow {
                chain: part.chain.clone(),
                actor: part.actor.to_string(),
                node: part.node.as_str().to_owned(),
                state: state.to_owned(),
                in_play: Some(state != "destroyed"),
                position,
            }
        })
        .collect();
    let ticks = app
        .world()
        .resource::<cs_app::physics::PhysicsTickLedger>()
        .ticks;
    teardown(&mut app);

    assert_eq!(operands.members.len(), M01_OPERAND_CHAINS);
    assert_eq!(
        rows.len(),
        M01_PART_CHAINS,
        "every spelled part-state chain got a live entity: {} bound",
        rows.len()
    );
    let workers = rows
        .iter()
        .filter(|row| row.chain.first().map(String::as_str) == Some("workersvoyagezep"))
        .count();
    let pirates = rows
        .iter()
        .filter(|row| row.chain.first().map(String::as_str) == Some("piratezep"))
        .count();
    assert_eq!(workers, M01_WORKERS_PART_CHAINS);
    assert_eq!(pirates, M01_PIRATE_PART_CHAINS);
    assert!(
        rows.iter()
            .all(|row| row.state == "intact" && row.in_play == Some(true)),
        "every part starts intact and in play"
    );

    // The record's own engine node names: the cross-check that the chains'
    // parts are the ones the zeppelins' records declare.
    let census = cs_content::pilots::survey_retail_zeppelin_carrier(game_dir)
        .expect("the production carrier census reads the installation");
    let m01 = census
        .missions()
        .iter()
        .find(|row| {
            row.group().eq_ignore_ascii_case("c1c") && row.mission().eq_ignore_ascii_case("m01")
        })
        .expect("M01 carries zeppelins.zrd");
    let member = m01.member().expect("M01's member decodes");
    let record_engines: usize = member
        .records()
        .iter()
        .map(|record| record.engines().len())
        .sum();

    let actor_level: Vec<String> = operands
        .members
        .iter()
        .filter(|chain| chain.len() == 1)
        .map(|chain| chain.join("/"))
        .collect();

    let mut unknowns = vec![
        "No per-engine integrity pool is stated anywhere in the installation: M01's \
         zeppelins.zrd records give floats for their gasbags (150.0 for workersvoyagezep's nine, \
         120.0 for piratezep's six) and the instant-action records carry cannon_health rows \
         (200.0), but nothing for an engine node, and the engine image's in-play-bit producers are \
         untraced (cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED). The part damage nodes therefore \
         carry a LABELLED DESIGNED single-unit pool \
         (m01-play-triggers-parts.designed-part-integrity), which is not a claim about the \
         original's engine hit points; affected content is every INACTIVE ladder over the 39 \
         chains; resolving task #668 (the measured damage table), F27-E."
            .to_owned(),
        "The two actor-level INACTIVE ladders (block indexes 53 and 55, the one-element chain \
         [piratezep]) read the zeppelin's OWN in-play bit, not a part's: they need the world \
         actor's destruction, whose producer is the same untraced writer and whose pool is the \
         unresolved one #1435 declared. Affected content is OBJECTIVE54/OBJECTIVE56; resolving \
         tasks #1435/#668 or a measured world-actor lifecycle rule."
            .to_owned(),
        "Nothing in the composed run destroys a zeppelin part today: M01's zeppelins.zrd records \
         state no team, so no target record declares them hostile and the F27 gun step refuses a \
         shot at them by name, and the parts carry no swept-box geometry (the per-node authored \
         transforms of non-object records are not decoded). The destruction path is therefore \
         exercised through the session damage authority the weapon and ordnance steps publish \
         into, and wiring a live geometric producer is affected content; resolving tasks \
         #1436/#1437 follow-ups and F30-E (allegiance)."
            .to_owned(),
        "The windowed firing criterion carried from #1439 is NOT met and not claimed here \
         (capabilities retail, synthetic only): the proximity trigger needs the player within \
         700 m of workersvoyagezep from a start 5 311 m away. Affected content is the windowed \
         M01 run; resolving task M01-PLAY-TRIGGERS-WINDOWED (#1486)."
            .to_owned(),
    ];
    unknowns.sort();

    PartRun {
        ticks_start: 0,
        ticks_end: ticks,
        chains: operands.members.len(),
        workers,
        pirates,
        record_engines,
        bound: rows,
        ladders,
        actor_level,
        unknowns,
    }
}

/// Every `InactiveMembers` arm the lowered program carries, in block order.
fn ladder_census(program: &cs_script::ir::MissionProgram) -> Vec<(usize, u32, usize)> {
    fn walk(index: usize, condition: &Condition, out: &mut Vec<(usize, u32, usize)>) {
        match condition {
            Condition::InactiveMembers { members, threshold } => {
                out.push((index, *threshold, members.len()));
            }
            Condition::All(inner) | Condition::Any(inner) => {
                for child in inner {
                    walk(index, child, out);
                }
            }
            Condition::Not(inner) => walk(index, inner, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (index, objective) in program.objectives.iter().enumerate() {
        walk(index, &objective.condition, &mut out);
    }
    out
}

/// The observation as JSON: measured rows and counts, no original bytes.
fn render_parts(observation: &PartRun, install_sha256: &str, candidate_tree: &str) -> String {
    let bound: Vec<String> = observation
        .bound
        .iter()
        .map(|row| {
            format!(
                "{{\"chain\": [{}], \"actor\": {}, \"node\": {}, \"part_state\": {}, \"in_play\": \
                 {}, \"position\": [{}, {}, {}]}}",
                row.chain
                    .iter()
                    .map(|name| jstr(name))
                    .collect::<Vec<_>>()
                    .join(", "),
                jstr(&row.actor),
                jstr(&row.node),
                jstr(&row.state),
                row.in_play
                    .map_or_else(|| "null".to_owned(), |value| value.to_string()),
                row.position[0],
                row.position[1],
                row.position[2],
            )
        })
        .collect();
    let ladders: Vec<String> = observation
        .ladders
        .iter()
        .map(|(block, threshold, members)| {
            format!("{{\"block\": {block}, \"threshold\": {threshold}, \"members\": {members}}}")
        })
        .collect();
    let actor_level: Vec<String> = observation
        .actor_level
        .iter()
        .map(|chain| jstr(chain))
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"ticks\": {{\"start\": {}, \"end\": \
         {}}}, \"spelled_member_chains\": {}, \"bound_part_chains\": {}, \"workers_parts\": {}, \
         \"pirate_parts\": {}, \"record_engine_nodes\": {}, \"bound_parts\": [{}], \"inactive_ladders\": \
         [{}], \"actor_level_chains\": [{}], \"unknowns\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.ticks_start,
        observation.ticks_end,
        observation.chains,
        observation.bound.len(),
        observation.workers,
        observation.pirates,
        observation.record_engines,
        bound.join(", "),
        ladders.join(", "),
        actor_level.join(", "),
        observation
            .unknowns
            .iter()
            .map(|entry| jstr(entry))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_triggers_parts.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-TRIGGERS-PARTS` written
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
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of this task's tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo ran,
/// so reading it would report hundreds of unrelated tests as this task's
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
                 \"part-states.json\"]}}",
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

fn unknown_array(unknowns: &[String]) -> String {
    unknowns
        .iter()
        .map(|entry| jstr(entry))
        .collect::<Vec<_>>()
        .join(", ")
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
