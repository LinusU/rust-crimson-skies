//! Evidence-report harness for task `M01-PLAY-TRIGGERS-OBSERVE` (#1480):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_triggers_observe_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_play_triggers_observe_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-OBSERVE/cargo-test.log
//!    ```
//!    (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-OBSERVE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_observe_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_observe -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-TRIGGERS-OBSERVE/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-TRIGGERS-OBSERVE --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-TRIGGERS-OBSERVE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness rebuilds M01's composition
//! headlessly, lets the installed observer write its report for the composed
//! ticks, and records what the host actually folded — member rows with their
//! observed positions, group rows with their live counts, the record's own
//! `OBJECTIVE3` effect lists and the `zeppelins.zrd` key census that shows
//! why no zeppelin group is ever reported. Names, numbers and digests only,
//! never original bytes.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionStage, PLAYER_MEMBER, build_headless, stage_for, teardown,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_observe_";

/// The measured shape this task's acceptance asserts — asserted verbatim by
/// the `accept_m01_play_triggers_observe_` tests, so a report regenerated on
/// an installation whose records disagree can never be written.
const M01_OPERAND_CHAINS: usize = 42;
const M01_ZEPPELINS: usize = 3;
const M01_SPAWNED_AIRCRAFT: usize = 9;

/// How this run was reviewed, with every measured number **derived** from
/// the production reads this same run produced rather than written down.
fn review_method(
    chains: usize,
    zeppelins: usize,
    groups: Vec<i32>,
    keys: usize,
    own_tests: usize,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         (world-observation.json): cs_app::mission_session::stage_for rebuilt M01's stage, \
         build_headless composed it, the installed observer mission_world_observation wrote its \
         report and MissionHost::step folded it, so the artifact records what the host really \
         observed — {chains} spelled member chains of which only the rows listed are observable, \
         {zeppelins} bound zeppelins, group ids {groups:?}, and {keys} stated zeppelins.zrd keys \
         from the production carrier census. Claim is implemented only. MEASURED: the TRAVELERS \
         anchor and subject spellings, the 700 m radius, the group ids and the engine effect \
         lists come from the record through the production readers; the group field on an \
         aiv.zrd record is field 4 of an 81-field positional grammar that zeppelins.zrd's keyed \
         records do not share. DESIGNED: which pose source an observer reads (the player's \
         physics Position, a binding entity's Transform) is this reimplementation's own \
         contract, never an original measurement; nothing here is verified_original and no \
         original executable ran. TEST-SELECTION NOTE: the prefix \
         accept_m01_play_triggers_observe_ is unique to this task, so the {own_tests} discovered \
         assertions are exactly this task's tests. candidate_tree is the tree of the commit the \
         acceptance suite and this harness both ran on; the only later delta on the branch is \
         this report's own copy under docs/findings/evidence/ plus the finding's prose. \
         Validated with tools/validate_evidence.py (structure, counts and artifact hashes; \
         --require-pass is not used because the report's unknowns must not be deleted)."
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_observe_writes_the_acceptance_report() {
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

    // The second production observation: the composed run's own world
    // observation, the record's effect lists and the zeppelin key census.
    let observation = observe_world(&game_dir);
    let observation_path = evidence_dir.join("world-observation.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_world_observation(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write world-observation.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-TRIGGERS-OBSERVE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": {}, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(
            observation.chains,
            observation.zeppelins,
            observation.groups,
            observation.zeppelin_keys,
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

/// What the second production observation records: what the composed run's
/// host really folded, and why the rows it did not fold are unobservable.
struct WorldObservationRun {
    /// The fixed ticks the composed run covered.
    ticks_start: u64,
    /// The last fixed tick the composed run reached.
    ticks_end: u64,
    /// How many of M01's spelled member chains the observer wrote rows for.
    observed_chains: usize,
    /// How many member chains the record spells in total.
    chains: usize,
    /// The member rows the host folded: chain, in play, position.
    member_rows: Vec<String>,
    /// The group ids the host folded, with their live counts.
    groups: Vec<i32>,
    /// The group rows, as `{"group": n, "living": n}`.
    group_rows: Vec<String>,
    /// The bound zeppelins the observer saw.
    zeppelins: usize,
    /// The keys M01's `zeppelins.zrd` records state, through the production
    /// carrier census.
    zeppelin_keys: usize,
    /// `OBJECTIVE3`'s own effect lists, read from the recovered engine model.
    nap_target: String,
    nap_time: String,
    wake_targets: String,
    kill_targets: String,
    /// The absences the report names instead of filling in.
    unknowns: Vec<String>,
}

/// Rebuilds M01's composition and records what the installed observer's
/// report made the host fold — a real production run over the owner's
/// installation, not a paraphrase of the acceptance assertions.
fn observe_world(game_dir: &Path) -> WorldObservationRun {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M01").expect("the label is valid"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage: MissionStage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    let chains = cs_app::world_facts::WorldOperands::of(&stage.host.control)
        .members
        .len();
    let engine = stage
        .host
        .engine_objectives
        .as_ref()
        .expect("M01 carries the recovered engine model");
    let declaration = engine.get(2).expect("OBJECTIVE3 loads").declaration.clone();

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    for _ in 0..4 {
        app.update();
    }
    let host = app.world().resource::<MissionHost>();
    let observation = host.world_facts().observation();
    let member_rows: Vec<String> = observation
        .members()
        .iter()
        .map(|(chain, row)| {
            format!(
                "{{\"chain\": [{}], \"in_play\": {}, \"position\": [{}, {}, {}]}}",
                chain
                    .iter()
                    .map(|name| jstr(name))
                    .collect::<Vec<_>>()
                    .join(", "),
                row.in_play,
                row.position[0],
                row.position[1],
                row.position[2],
            )
        })
        .collect();
    let groups: Vec<i32> = observation.groups().keys().copied().collect();
    let group_rows: Vec<String> = observation
        .groups()
        .iter()
        .map(|(group, living)| format!("{{\"group\": {group}, \"living\": {living}}}"))
        .collect();
    let ticks = app
        .world()
        .resource::<cs_app::physics::PhysicsTickLedger>()
        .ticks;
    let spawned = app
        .world()
        .resource::<cs_app::mission_actors::MissionActorSpawnReport>()
        .aircraft
        .len();
    teardown(&mut app);

    assert_eq!(
        member_rows.len(),
        4,
        "the composed run folds the player and the three zeppelins, and no other chain: \
         {member_rows:?}"
    );
    assert_eq!(
        member_rows
            .iter()
            .filter(|row| row.contains(&jstr(PLAYER_MEMBER)))
            .count(),
        1,
        "the player row exists exactly once"
    );
    assert_eq!(
        groups,
        vec![1, 3, 5],
        "the composition spawned three groups' members: {groups:?}"
    );
    assert_eq!(spawned, M01_SPAWNED_AIRCRAFT);
    assert_eq!(chains, M01_OPERAND_CHAINS);

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
    assert_eq!(member.records().len(), M01_ZEPPELINS);
    let zeppelin_keys: usize = member
        .records()
        .iter()
        .map(|record| record.keys().len())
        .sum();

    let mut unknowns = vec![
        "M01's 39 part-state chains (workersvoyagezep/piratezep part states) have no entity in \
         the composed world, so this observer cannot report them: affected content is every \
         INACTIVE ladder over those chains; resolving task M01-PLAY-TRIGGERS-PARTS (#1481)."
            .to_owned(),
        "DEDG groups 2 and 4 have no spawned member (their only aiv.zrd records are the measured \
         enabled == 0 skips), so no count is ever reported for them: affected content is the \
         eight DEDG sites over groups 1..=4; resolving task M01-PLAY-TRIGGERS-DEPLETION (#1483)."
            .to_owned(),
        "The three ANIM_STATE animations have no producer (WAKE_ANIM has no runtime consumer), \
         so no animation row is observed: affected content is OBJECTIVE11/15/18; resolving task \
         M01-PLAY-TRIGGERS-ANIMS (#1482)."
            .to_owned(),
        "The windowed firing criterion carried from #1439 is NOT met and not claimed here \
         (capabilities retail, synthetic only): cs --mission takes no input replay or start-pose \
         override and its capture script is fixed at one second of pitch input, while the mission \
         starts 5 311 m from the anchor against a 700 m radius; affected content is the windowed \
         M01 proximity trigger; resolving task M01-PLAY-TRIGGERS-WINDOWED (#1486)."
            .to_owned(),
    ];
    unknowns.sort();

    WorldObservationRun {
        ticks_start: 0,
        ticks_end: ticks,
        observed_chains: member_rows.len(),
        chains,
        member_rows,
        groups,
        group_rows,
        zeppelins: M01_ZEPPELINS,
        zeppelin_keys,
        nap_target: format!("{:?}", declaration.nap.and_then(|nap| nap.target)),
        nap_time: format!("{:?}", declaration.nap.map(|nap| nap.time)),
        wake_targets: format!("{:?}", declaration.wake.targets()),
        kill_targets: format!("{:?}", declaration.kill.targets()),
        unknowns,
    }
}

/// The observation as JSON: measured rows and counts, no original bytes.
fn render_world_observation(
    observation: &WorldObservationRun,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"ticks\": {{\"start\": {}, \"end\": \
         {}}}, \"spelled_member_chains\": {}, \"observed_member_chains\": {}, \"member_rows\": \
         [{}], \"groups\": [{}], \"group_rows\": [{}], \"zeppelins\": {}, \"zeppelins_zrd_keys\": \
         {}, \"objective3\": {{\"nap_target_index\": {}, \"nap_seconds\": {}, \"wake_target_indexes\": \
         {}, \"kill_target_indexes\": {}}}, \"unknowns\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.ticks_start,
        observation.ticks_end,
        observation.chains,
        observation.observed_chains,
        observation.member_rows.join(", "),
        observation
            .groups
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        observation.group_rows.join(", "),
        observation.zeppelins,
        observation.zeppelin_keys,
        observation.nap_target,
        observation.nap_time,
        observation.wake_targets,
        observation.kill_targets,
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
             (crates/cs_app/tests/evidence_report_m01_play_triggers_observe.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-TRIGGERS-OBSERVE` written
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

/// Extracts the per-test results of the `accept_m01_play_triggers_observe_`
/// tests from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with
/// the schema's `unknown` status rather than counted as a pass, so a report
/// can never claim an assertion it did not run.
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
                 \"world-observation.json\"]}}",
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
