//! Evidence-report harness for task `M01-PLAY-TRIGGERS-ACTOR-BIT` (#1490):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_triggers_actor_bit_*`: it is not part of the
//! acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_play_triggers_actor_bit_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT/cargo-test.log
//!    ```
//!    (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_actor_bit_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_actor_bit -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT`
//!    (exit 0, `structurally_valid`). `--require-pass` is deliberately not
//!    used: it refuses any report whose `unknowns` array is non-empty
//!    (`tools/validate_evidence.py`), and this report carries the unresolved
//!    issues the owner's 2026-09-28 directive forbids deleting — the untraced
//!    in-play-bit producers, the unreachable world-actor `Destroy` arm in a
//!    composed run, the unresolved zeppelin pool, block 55's wake chain behind
//!    `OBJECTIVE22` and #1439's windowed criterion. The pass/fail facts are
//!    carried by `command.exit_code` and the `tests` counts, which the plain
//!    check does evaluate.
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness rebuilds M01's composition
//! headlessly and records, per bound zeppelin, the world-actor session's own
//! lifecycle presence, the damage authority's destruction ledger and the
//! observer's in-play row — beside the actor-level `INACTIVE` ladder census
//! read from the lowered program. Names, numbers and digests only, never
//! original bytes.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_actors::MissionActorSpawnReport;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionStage, build_headless, stage_for, subject_node_name, teardown,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_facts::WorldOperands;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_script::ir::Condition;
use cs_sim::world_actors::graph::Presence;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_actor_bit_";

/// The measured shape this task's acceptance asserts — asserted verbatim by
/// the `accept_m01_play_triggers_actor_bit_` tests, so a report regenerated
/// on an installation whose records disagree can never be written.
const M01_ZEPPELINS: usize = 3;
const M01_ACTOR_LEVEL_LADDERS: usize = 2;

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_actor_bit_writes_the_acceptance_report() {
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

    let observation = observe_actor_bit(&game_dir);
    let observation_path = evidence_dir.join("actor-bit.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_actor_bit(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write actor-bit.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-TRIGGERS-ACTOR-BIT\", \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": {}, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
fn review_method(observation: &ActorBitRun, own_tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         (actor-bit.json): cs_app::mission_session::stage_for rebuilt M01's stage, build_headless \
         composed it, the world-actor session launched the {zeppelins} zeppelins, the spawn host \
         bound each an ECS entity, and MissionHost::step folded the observer's rows — every actor \
         row's presence is the world-actor session's lifecycle crossed with the session damage \
         authority's destruction ledger (mission_session::world_actor_in_play), never a default. \
         The {ladders} actor-level INACTIVE ladders over the one-element chain [piratezep] are \
         read from the lowered program. Claim is implemented only. MEASURED: the bindings, the \
         lifecycle presence, the ledger state, the observer rows and the ladder census all come \
         from the production readers on this run. DESIGNED: nothing new this task — the out-of-play \
         row is a read of the two existing measured fact families, and no integrity pool was \
         invented. What the ORIGINAL does to clear the zeppelin's own in-play bit stays untraced \
         (cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED). Nothing here is verified_original and no \
         original executable ran. TEST-SELECTION NOTE: the prefix {prefix} is unique to this task, \
         so the {own_tests} discovered assertions are exactly this task's tests. candidate_tree is \
         the tree of the commit the acceptance suite and this harness both ran on; the only later \
         delta on the branch is this report's own copy under docs/findings/evidence/ plus the \
         finding's prose. Validated with tools/validate_evidence.py (structure, counts and \
         artifact hashes; --require-pass is not used because the report's unknowns must not be \
         deleted).",
        zeppelins = observation.zeppelins.len(),
        ladders = observation.ladders.len(),
        prefix = ACCEPTANCE_PREFIX,
    )
}

/// What the second production observation records.
struct ActorBitRun {
    ticks_start: u64,
    ticks_end: u64,
    /// Every bound zeppelin with its world's-own-state reads.
    zeppelins: Vec<ZeppelinRow>,
    /// The actor-level `INACTIVE` ladders: `(block, threshold, members)`.
    ladders: Vec<(usize, u32, usize)>,
    /// The absences the report names instead of filling in.
    unknowns: Vec<String>,
}

/// One bound zeppelin as the second observation records it.
struct ZeppelinRow {
    name: String,
    world_actor: u32,
    damage_actor: String,
    presence: String,
    ledger_destroyed: bool,
    observed_in_play: Option<bool>,
}

/// Rebuilds M01's composition and records what the world's own state holds
/// for each bound zeppelin — a real production run over the owner's
/// installation, not a paraphrase of the acceptance assertions.
fn observe_actor_bit(game_dir: &Path) -> ActorBitRun {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M01").expect("the label is valid"),
        "The Lost Treasure",
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
        .get_resource::<MissionActorSpawnReport>()
        .expect("the spawn host installed its report")
        .clone();
    let rows: Vec<ZeppelinRow> = report
        .zeppelins
        .iter()
        .map(|zeppelin| {
            let name = subject_node_name(&zeppelin.subject)
                .unwrap_or("<unreadable>")
                .to_owned();
            let presence = app
                .world()
                .resource::<MissionHost>()
                .world_actors()
                .and_then(|session| session.set().presence(zeppelin.world_actor))
                .map_or_else(
                    || "unheld".to_owned(),
                    |state| match state {
                        Presence::Intact => "intact".to_owned(),
                        Presence::Destroyed => "destroyed".to_owned(),
                    },
                );
            let ledger_destroyed = app
                .world()
                .resource::<SessionDamageResolver>()
                .resolver()
                .is_destroyed(&zeppelin.actor);
            let observed_in_play = app
                .world()
                .resource::<MissionHost>()
                .world_facts()
                .observation()
                .members()
                .get(&cs_app::world_facts::member_chain([name.as_str()]))
                .map(|row| row.in_play);
            ZeppelinRow {
                name,
                world_actor: zeppelin.world_actor.0,
                damage_actor: zeppelin.actor.to_string(),
                presence,
                ledger_destroyed,
                observed_in_play,
            }
        })
        .collect();
    let ticks = app
        .world()
        .resource::<cs_app::physics::PhysicsTickLedger>()
        .ticks;
    teardown(&mut app);

    assert_eq!(rows.len(), M01_ZEPPELINS, "M01 places three zeppelins");
    assert!(
        rows.iter().all(|row| row.presence == "intact"),
        "every zeppelin starts held intact by its session: {:?}",
        rows.iter().map(|row| &row.presence).collect::<Vec<_>>()
    );
    assert!(
        rows.iter().all(|row| !row.ledger_destroyed),
        "no zeppelin's destruction is recorded at the start"
    );
    assert!(
        rows.iter().all(|row| row.observed_in_play == Some(true)),
        "every bound actor row is observed in play before any destruction"
    );
    assert!(
        ladders.len() >= M01_ACTOR_LEVEL_LADDERS,
        "the lowered program carries the actor-level ladders: {ladders:?}"
    );
    // The actor-level operand chains are the one-element ones — the zeppelin
    // nodes themselves, never a part. M01's are exactly the two ladders read
    // above, over ["piratezep"].
    let actor_level: Vec<String> = operands
        .members
        .iter()
        .filter(|chain| chain.len() == 1)
        .map(|chain| chain.join("/"))
        .collect();
    assert!(
        actor_level.contains(&"piratezep".to_owned()),
        "the one-element operands name the zeppelins themselves: {actor_level:?}"
    );

    let mut unknowns = vec![
        "The original's in-play-bit producers (+0x24 bit 4, gwNodeSetActive at VA 0x4cca30) stay \
         untraced (cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED): WHAT the original does to clear \
         a zeppelin's own node is unmeasured. Affected content is OBJECTIVE54/OBJECTIVE56's \
         semantics in the original; resolving task #1360's open item and an original run (#358)."
            .to_owned(),
        "The composed host steps the world-actor session with NO commands (MissionHost::step \
         stage 2 schedules nothing on its behalf) and exposes no command port, so the session's \
         Destroy arm — Presence::Destroyed — cannot fire inside a composed run today. The \
         observer reads it (it is the world's own lifecycle state), but the drivable out-of-play \
         route is the damage authority's destruction ledger. Affected content is any mission \
         that should destroy a world actor from the mission side; resolving: a world-actor \
         command port (F34 wiring) or a measured damage rule for zeppelins."
            .to_owned(),
        "The zeppelin damage graph's single airframe node keeps the pool #1435 declared \
         (Resolved::Unknown, #668/F27-E): a routed hit blocks, so no hit-produced destruction of \
         a zeppelin is possible today and no integrity pool was invented here. Affected content \
         is a live gun kill of a zeppelin; resolving task #668 (the measured damage table)."
            .to_owned(),
        "OBJECTIVE56 (block 55) is woken only by block 21 (OBJECTIVE22), which starts Dormant \
         (measured from M01's recovered declarations): this suite does not drive that wake chain, \
         so the block's COMPLETION is gated even though its ladder condition holds from this \
         task's actor row. Affected content is OBJECTIVE56's completion in a live M01 run; \
         resolving: the M01-B/F39 wake-chain work over OBJECTIVE22."
            .to_owned(),
        "The windowed firing criterion carried from #1439 is NOT met and not claimed here \
         (capabilities retail, synthetic only): the proximity trigger needs the player within \
         700 m of workersvoyagezep from a start 5 311 m away. Affected content is the windowed \
         M01 run; resolving task M01-PLAY-TRIGGERS-WINDOWED (#1486)."
            .to_owned(),
    ];
    unknowns.sort();

    ActorBitRun {
        ticks_start: 0,
        ticks_end: ticks,
        zeppelins: rows,
        ladders,
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
fn render_actor_bit(
    observation: &ActorBitRun,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let zeppelins: Vec<String> = observation
        .zeppelins
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"world_actor\": {}, \"damage_actor\": {}, \"session_presence\": \
                 {}, \"ledger_destroyed\": {}, \"observed_in_play\": {}}}",
                jstr(&row.name),
                row.world_actor,
                jstr(&row.damage_actor),
                jstr(&row.presence),
                row.ledger_destroyed,
                row.observed_in_play
                    .map_or_else(|| "null".to_owned(), |value| value.to_string()),
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
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"ticks\": {{\"start\": {}, \"end\": \
         {}}}, \"bound_zeppelins\": [{}], \"inactive_ladders\": [{}], \"unknowns\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.ticks_start,
        observation.ticks_end,
        zeppelins.join(", "),
        ladders.join(", "),
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
             (crates/cs_app/tests/evidence_report_m01_play_triggers_actor_bit.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT` written
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
    let year = era * 400 + day_of_era / 1461 + day_of_era / 365 - day_of_era / 146_096;
    let day_of_year =
        day_of_era - (365 * day_of_era / 1461 + day_of_era / 36_524 - day_of_era / 146_096);
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
                 \"actor-bit.json\"]}}",
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
