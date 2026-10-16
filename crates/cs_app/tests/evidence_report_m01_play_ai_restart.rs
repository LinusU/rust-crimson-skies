//! Evidence-report harness for task `M01-PLAY-AI-RESTART` (#1509):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_ai_restart_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_play_ai_restart_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-AI-RESTART/cargo-test.log
//!    ``` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-AI-RESTART \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_ai_restart_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_ai_restart -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-AI-RESTART/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-AI-RESTART`
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-AI-RESTART.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-reads M01's stage
//! through `stage_for`, composes the mission headlessly, records the spawn's
//! flown/held split and each flown record's session actor, then drives the
//! composed restart entry (a `MissionHostRestartRequest` consumed by
//! `app.update()`) and records the fresh generation's split — every flown
//! record back with its law, its designed behaviour and a freshly allocated
//! session actor, the measured sleeper still held. That is a real production
//! run over the owner's installation, not a paraphrase of the acceptance
//! assertions, and it carries no original bytes: record names, counts and
//! session ids only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::{App, Entity};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::{MissionActorSpawnReport, ScriptedAircraftActor};
use cs_app::mission_ai::{MissionAiReport, MissionAircraftAi};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHostRestartReport, MissionHostRestartRequest, MissionStage, build_headless, stage_for,
    teardown,
};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::damage::ActorId;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_ai_restart_";

/// The measured census the acceptance suite asserts, so a report regenerated
/// on an installation whose records disagree can never be written.
const M01_RECORD_COUNT: usize = 12;
const M01_FLOWN_COUNT: usize = 8;
const M01_HELD_COUNT: usize = 1;

/// The one record M01's measured `deactivated` field sleeps.
const SLEEPER: &str = "bsfury_1";

/// How this run was reviewed, with every number **derived** from the
/// production reads this same run produced rather than written down.
#[allow(clippy::too_many_arguments)]
fn review_method(
    records: usize,
    flown_before: usize,
    held_before: usize,
    flown_after: usize,
    held_after: usize,
    fresh_actors: usize,
    refusals: usize,
    own_tests: usize,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production run (ai-restart.json): stage_for re-read M01 through the production \
         readers, the composed mission was spawned ({records} measured records, {flown_before} \
         flown and {held_before} held), the composed restart entry was consumed through a \
         MissionHostRestartRequest, and the fresh generation spawned {flown_after} flown and \
         {held_after} held with {fresh_actors} freshly allocated session actors and {refusals} \
         AI refusals. \
         TEST-SELECTION NOTE: the prefix {ACCEPTANCE_PREFIX} is unique to this task, so the \
         {own_tests} discovered assertions are exactly this task's tests. MEASURED: the aiv.zrd \
         record fields (#1453) including the sleep field (#681), and the stage's imported \
         airframe table (#796, OWNER-STATIC-2026-10-08, never verified_original). DESIGNED: the \
         behaviour each record's measured fields select and the pursuit-and-fire law itself \
         (the original's control law 0x48c220 is unrecovered), the body swap the respawn makes, \
         and the restart's session allocation. Validated with tools/validate_evidence.py (see \
         the unknowns: with the unrecovered AI control law still declared, --require-pass \
         reports Unresolved issues, exactly as docs/findings/evidence/M01-PLAY-AI.json records \
         for its own pinned unknowns)."
    )
}

/// What the report still cannot claim, each with the content it affects, the
/// task that resolves it and the claim it gates (owner directive,
/// 2026-10-05).
fn open_unknowns() -> Vec<String> {
    vec![
        "original_ai_control_law: a respawned record flies the same designed pursuit-and-fire \
         law every awake record flies, because the original's AI control law at 0x48c220 was \
         never recovered. Affected content: every scripted aircraft's manoeuvre, aim and \
         throttle in any mission, before and after any restart. Resolving task: static analysis \
         of 0x48c220's callers, or an owner original run (#358 REF-OWNER-FIRST-CAPTURE). Gates: \
         any fidelity claim about how a scripted aircraft flies."
            .to_owned(),
        "ai_parameter_randomisation: the original rolls every per-airframe parameter by +/-5% \
         for AI aircraft (0x4771c6) and never for the player; the respawn applies no roll, so \
         a respawned airframe flies the exact retail values. Affected content: every AI \
         aircraft's handling envelope. Resolving task: #805 \
         FLIGHT-ORIGINAL-AI-RANDOMISATION. Gates: any claim that AI aircraft handle like the \
         original's."
            .to_owned(),
        "team_and_primary_target_semantics: what the aiv.zrd team numbers mean engine-wide, and \
         what the original did with a stored primary target, are unmeasured; the behaviour a \
         respawned record takes is the same designed reading of those measured fields the spawn \
         takes. Affected content: who every scripted aircraft attacks in every mission record \
         set. Resolving task: #1453's follow-ups or an owner original run. Gates: any claim \
         about the original's targeting choices."
            .to_owned(),
        "restart_behaviour_reference: whether the original's own retry/restart respawned its \
         scripted aircraft flying is unmeasured — no original run exists (#358), so this \
         observation pins this implementation's restart contract (the authored initial state, \
         freshly allocated session actors) and never the original's behaviour. Affected \
         content: M01's retry path and every mission restart. Resolving task: \
         REF-OWNER-FIRST-CAPTURE or an owner human_review of a restart. Gates: any \
         verified_original claim about restarts."
            .to_owned(),
    ]
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_ai_restart_writes_the_acceptance_report() {
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

    // The second production observation: re-read the stage, spawn it, restart
    // it through the composed entry, and read the fresh generation back.
    let observation = observe_restart(&game_dir);
    let observation_path = evidence_dir.join("ai-restart.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_ai_restart(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write ai-restart.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let unknowns = open_unknowns();
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-AI-RESTART\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        observation.ticks,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        unknowns
            .iter()
            .map(|unknown| jstr(unknown))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&reviewer),
        jstr(&review_method(
            observation.records,
            observation.flown_before,
            observation.held_before,
            observation.flown_after,
            observation.held_after,
            observation.rows.len(),
            observation.refusals_after.len(),
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

/// One flown record, before and after the composed restart.
#[derive(Debug)]
struct RestartRow {
    /// The measured record name.
    record: String,
    /// The designed behaviour the spawn selected.
    behaviour_before: String,
    /// The designed behaviour after the restart.
    behaviour_after: String,
    /// The first generation's session actor (`session:serial`).
    actor_before: String,
    /// The fresh generation's session actor (`session:serial`).
    actor_after: String,
    /// Whether the two actors differ — the fresh allocation.
    fresh: bool,
}

/// What the second production observation records about the restart.
struct RestartObservation {
    /// The records `aiv.zrd` carries.
    records: usize,
    /// Records flying at the first spawn.
    flown_before: usize,
    /// Records held at the first spawn.
    held_before: usize,
    /// Records flying after the composed restart.
    flown_after: usize,
    /// Records held after the composed restart.
    held_after: usize,
    /// Committed fixed ticks driven after the restart.
    ticks: u64,
    /// One row per flown record.
    rows: Vec<RestartRow>,
    /// Every AI refusal the spawn after the restart reported.
    refusals_after: Vec<String>,
}

/// Re-reads M01's stage and drives the composed mission headlessly through
/// its composed restart entry — a real production run over the owner's
/// installation.
fn observe_restart(game_dir: &Path) -> RestartObservation {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M01").expect("the label is valid"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage: MissionStage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    assert_eq!(
        stage.aircraft.len(),
        M01_RECORD_COUNT,
        "M01's aiv.zrd no longer carries the measured twelve records"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // The spawn-only frame: zero fixed ticks run, so the roster is exactly
    // what the first spawn made.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    let spawn_report = app.world().resource::<MissionAiReport>().clone();
    assert_eq!(
        spawn_report.flown.len(),
        M01_FLOWN_COUNT,
        "the composed run flies the awake eight"
    );
    assert_eq!(
        spawn_report.held.len(),
        M01_HELD_COUNT,
        "exactly the sleeper is held at the spawn"
    );
    assert!(
        spawn_report
            .held
            .iter()
            .any(|held| held.starts_with(SLEEPER)),
        "the one held record is the measured `deactivated` sleeper {SLEEPER}: {:?}",
        spawn_report.held
    );
    assert!(
        spawn_report.refusals.is_empty(),
        "the first spawn reported no AI refusals: {:?}",
        spawn_report.refusals
    );
    let mut flown: Vec<String> = spawn_report.flown.clone();
    flown.sort();
    let before: Vec<(String, Entity, String, String)> = flown
        .iter()
        .map(|name| {
            let entity = entity_named(&mut app, name);
            let ai = app
                .world()
                .get::<MissionAircraftAi>(entity)
                .unwrap_or_else(|| panic!("{name} spawns with its AI"));
            (
                name.clone(),
                entity,
                format!("{:?}", ai.behaviour),
                actor_label(&spawned(&app, name).actor),
            )
        })
        .collect();

    // The composed restart, exactly as the playtest reset and a terminal's
    // retry reach it. The respawn's own `MissionAiReport` is read here, right
    // after the restart frame and before any fixed tick steps the fresh
    // generation: that report is the spawn's, so every refusal in it is one
    // the respawn itself produced. (A driven tick would append the *unrelated*
    // one-tick weapon-session re-open ordering — `arm_mission_ai` runs before
    // `ensure_mission_weapons` re-opens the fresh generation's session in the
    // same tick — which is not this task's claim. See
    // docs/findings/2026-10-16-m01-play-ai-restart.md.)
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );

    let report = app.world().resource::<MissionAiReport>().clone();
    let mut flown_after: Vec<String> = report.flown.clone();
    flown_after.sort();
    assert_eq!(
        flown_after, flown,
        "every record that flew before the restart flies after it"
    );
    assert_eq!(
        report.held.len(),
        M01_HELD_COUNT,
        "the measured sleeper is still the one held record after the restart"
    );
    assert!(
        report.held.iter().any(|held| held.starts_with(SLEEPER)),
        "the one held record after the restart is still the measured sleeper {SLEEPER}: {:?}",
        report.held
    );
    assert!(
        report.refusals.is_empty(),
        "the respawn reported no AI refusals: {:?}",
        report.refusals
    );

    let rows: Vec<RestartRow> = before
        .iter()
        .map(|(name, first_entity, behaviour_before, actor_before)| {
            let entity = entity_named(&mut app, name);
            assert_ne!(
                entity, *first_entity,
                "{name} respawns as a fresh entity of the fresh generation"
            );
            let ai = app
                .world()
                .get::<MissionAircraftAi>(entity)
                .unwrap_or_else(|| panic!("{name} comes back with its AI"));
            let body = app
                .world()
                .get::<avian3d::prelude::RigidBody>(entity)
                .expect("the respawned body carries its kind");
            assert!(
                matches!(body, avian3d::prelude::RigidBody::Dynamic),
                "{name} comes back as the dynamic body the law moves"
            );
            assert!(
                !app.world()
                    .get::<PlaytestOriginalFlight>(entity)
                    .unwrap_or_else(|| panic!("{name} comes back with the law"))
                    .is_player,
                "{name} comes back on the law's AI branch, never the player's"
            );
            let actor_after = actor_label(&spawned(&app, name).actor);
            RestartRow {
                record: name.clone(),
                behaviour_before: behaviour_before.clone(),
                behaviour_after: format!("{:?}", ai.behaviour),
                actor_before: actor_before.clone(),
                fresh: actor_after != *actor_before,
                actor_after,
            }
        })
        .collect();
    assert!(
        rows.iter().all(|row| row.fresh),
        "every respawned record carries a freshly allocated session actor: {rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| row.behaviour_after == row.behaviour_before),
        "every respawned record takes the same designed behaviour: {rows:?}"
    );
    teardown(&mut app);

    RestartObservation {
        records: stage.aircraft.len(),
        flown_before: before.len(),
        held_before: M01_HELD_COUNT,
        flown_after: report.flown.len(),
        held_after: report.held.len(),
        ticks: 0,
        rows,
        refusals_after: report.refusals.clone(),
    }
}

/// The live entity of a scripted aircraft, by measured record name.
fn entity_named(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
    query
        .iter(world)
        .find(|(_, actor)| actor.record.name == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// The spawn report's entry for one record.
fn spawned(app: &App, name: &str) -> cs_app::mission_actors::SpawnedAircraft {
    app.world()
        .resource::<MissionActorSpawnReport>()
        .aircraft
        .iter()
        .find(|spawned| spawned.record == name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// A session actor as its `session:serial` spelling — no bytes, no ids of
/// anything else.
fn actor_label(actor: &ActorId) -> String {
    format!("{}:{}", actor.session.get(), actor.serial)
}

/// The observation as JSON: measured rows and the run's own numbers, no
/// original bytes — record names, counts and session spellings only.
fn render_ai_restart(
    observation: &RestartObservation,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let rows: Vec<String> = observation
        .rows
        .iter()
        .map(|row| {
            format!(
                "{{\"record\": {}, \"behaviour_before\": {}, \"behaviour_after\": {}, \
                 \"actor_before\": {}, \"actor_after\": {}, \"fresh\": {}}}",
                jstr(&row.record),
                jstr(&row.behaviour_before),
                jstr(&row.behaviour_after),
                jstr(&row.actor_before),
                jstr(&row.actor_after),
                row.fresh
            )
        })
        .collect();
    let refusals: Vec<String> = observation
        .refusals_after
        .iter()
        .map(|value| jstr(value))
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"zbd/c1c/m01 aiv.zrd + \
         objectives.zrd\", \"records\": {}, \"flown_before\": {}, \"held_before\": {}, \
         \"flown_after\": {}, \"held_after\": {}, \"post_restart_ticks\": {}, \"rows\": [{}], \
         \"refusals_after\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.records,
        observation.flown_before,
        observation.held_before,
        observation.flown_after,
        observation.held_after,
        observation.ticks,
        rows.join(", "),
        refusals.join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_ai_restart.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-AI-RESTART` written relative
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

/// Extracts the per-test results of the `accept_m01_play_ai_restart_` tests
/// from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this
/// task's acceptance selection. A prefixed test that was skipped is recorded
/// with the schema's `unknown` status rather than counted as a pass.
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

/// Whether a libtest name is one of this task's tests. The prefix is matched
/// on the name's **last** path segment, because libtest prints an in-module
/// unit test under its module path.
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
                 \"ai-restart.json\"]}}",
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
