//! Evidence-report harness for task `M01-PLAY-AI-WAKE` (#1487):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_ai_wake_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_play_ai_wake_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-AI-WAKE/cargo-test.log
//!    ``` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-AI-WAKE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_ai_wake_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_ai_wake -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-AI-WAKE/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-AI-WAKE`
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-AI-WAKE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-reads M01's stage
//! through `stage_for`, composes the mission headlessly, records that
//! `bsfury_1` spawns held, drives the record's own win chain until its
//! `WAKEUP_ENEMIES` site releases the sleeper, records what the host's
//! `WakeEnemiesLedger` consumed and refused, then drives the released record
//! and records how far it flew. That is a real production run over the
//! owner's installation, not a paraphrase of the acceptance assertions, and
//! it carries no original bytes: record names, chain spellings, counts and
//! metres only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use avian3d::prelude::Position;
use bevy::prelude::{App, Entity, Time};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::{MissionActorSpawnReport, ScriptedAircraftActor};
use cs_app::mission_ai::{MissionAiBehaviour, MissionAiReport, MissionAircraftAi};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionStage, MissionWorldObservation, build_headless, stage_for, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::world_facts::{MemberObservation, WorldObservation, member_chain};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_ai_wake_";

/// The measured census the acceptance suite asserts, so a report regenerated
/// on an installation whose records disagree can never be written.
const M01_RECORD_COUNT: usize = 12;
const M01_FLOWN_COUNT: usize = 8;
const M01_HELD_COUNT: usize = 1;

/// The sleeper the wake releases.
const SLEEPER: &str = "bsfury_1";

/// The non-aircraft name M01's wake site spells beside it.
const ZEPPELIN_NAME: &str = "blackswanzep";

/// The elapsed one driven retail step covers: one second of mission clock per
/// host step, the compression `accept_m01_b_wake.rs` documents. The release
/// lands on the `OBJECTIVE13` wake, so the compression changes only how fast
/// the chain's states arrive, never which ones do.
const DRIVEN_STEP: Duration = Duration::from_secs(1);

/// How many committed fixed ticks the second observation drives the released
/// record before it records how far it flew.
const FLY_TICKS: u64 = 3 * 60;

/// How this run was reviewed, with every number **derived** from the
/// production reads this same run produced rather than written down.
#[allow(clippy::too_many_arguments)]
fn review_method(
    records: usize,
    held_before: bool,
    released: usize,
    refused: usize,
    driven_ticks: u64,
    decisions: u64,
    flown_m: f32,
    own_tests: usize,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production run (wake-release.json): stage_for re-read M01 through the production \
         readers, the composed mission was spawned ({records} measured records), bsfury_1 was \
         found held at the spawn ({held_before}), the record's own win chain was driven until its \
         WAKEUP_ENEMIES site was emitted ({driven_ticks} committed ticks), the host released \
         {released} scripted aircraft and refused {refused} name that is not one, and the \
         released record then flew {flown_m:.1} m over {FLY_TICKS} committed ticks, with \
         {decisions} decisions written. \
         TEST-SELECTION NOTE: the prefix {ACCEPTANCE_PREFIX} is unique to this task, so the \
         {own_tests} discovered assertions are exactly this task's tests. MEASURED: the \
         aiv.zrd sleep field (#681) and record fields (#1453), the WAKEUP_ENEMIES site \
         OBJECTIVE13 spells ([text bsfury_1, text blackswanzep], accept_m01_lc_directive_c), the \
         measured directive semantics (wake a named vehicle only when its asleep byte is set, a \
         named zeppelin only when its dormant byte is set, do nothing for a name already awake), \
         and every per-airframe flight parameter the law consumes (#796, OWNER-STATIC-2026-10-08, \
         never verified_original). DESIGNED: the behaviour the woken record takes (the reading of \
         `team` and `primary_target` with the sleep consumed), the pursuit-and-fire law itself \
         (0x48c220 unrecovered), and the release's body swap — the same body an awake record \
         spawns as. Validated with tools/validate_evidence.py (see the unknowns: with the \
         unrecovered AI control law still declared, --require-pass reports Unresolved issues, \
         exactly as docs/findings/evidence/M01-PLAY-AI.json records for its own pinned unknowns)."
    )
}

/// What the report still cannot claim, each with the content it affects, the
/// task that resolves it and the claim it gates (owner directive,
/// 2026-10-05).
fn open_unknowns(flown_m: f32) -> Vec<String> {
    vec![
        "original_ai_control_law: a released record flies the same designed pursuit-and-fire law \
         every awake record flies, because the original's AI control law at 0x48c220 was never \
         recovered. Affected content: every released aircraft's manoeuvre, aim and throttle in \
         any mission. Resolving task: static analysis of 0x48c220's callers, or an owner original \
         run (#358 REF-OWNER-FIRST-CAPTURE). Gates: any fidelity claim about how a woken \
         aircraft flies."
            .to_owned(),
        "ai_parameter_randomisation: the original rolls every per-airframe parameter by +/-5% for \
         AI aircraft (0x4771c6) and never for the player; the release applies no roll, so a woken \
         airframe flies the exact retail values. Affected content: every AI aircraft's handling \
         envelope. Resolving task: #805 FLIGHT-ORIGINAL-AI-RANDOMISATION. Gates: any claim that \
         AI aircraft handle like the original's."
            .to_owned(),
        "team_and_primary_target_semantics: what the aiv.zrd team numbers mean engine-wide, and \
         what the original did with a stored primary target, are unmeasured; the behaviour the \
         woken record takes (opposed teams, primary target as the aircraft to engage) is a \
         designed reading of those measured fields. Affected content: who the woken record \
         attacks in every mission record set. Resolving task: #1453's follow-ups or an owner \
         original run. Gates: any claim about the original's targeting choices."
            .to_owned(),
        "zeppelin_wake_unmodelled: a WAKEUP_ENEMIES name that resolves to a zeppelin (M01's \
         blackswanzep) is refused by name here, because the world-actor wake (its dormant byte and \
         route) has no consumer. Affected content: the award/zeppelin that M01's OBJECTIVE13 also \
         wakes. Resolving task: a world-actor wake surface (F35-D). Gates: any claim that M01's \
         full OBJECTIVE13 wake is consumed."
            .to_owned(),
        format!(
            "observation_scope: the second observation drives {FLY_TICKS} committed ticks after the \
             release and records {flown_m:.1} m of flight; it is a production run, not an original \
             reference — no original capture exists for AI behaviour (#358), and the original's own \
             release timing and pose after a wake are unmeasured. Affected content: every \
             comparison against the original game. Resolving task: REF-OWNER-FIRST-CAPTURE. Gates: \
             any verified_original claim."
        ),
    ]
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_ai_wake_writes_the_acceptance_report() {
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

    // The second production observation: re-read the stage, watch the wake
    // release the sleeper, and fly it.
    let observation = observe_wake_release(&game_dir);
    let observation_path = evidence_dir.join("wake-release.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_wake_release(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write wake-release.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let unknowns = open_unknowns(observation.flown_m);
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-AI-WAKE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
            observation.held_before,
            observation.released_rows.len(),
            observation.refused_rows.len(),
            observation.ticks,
            observation.decisions,
            observation.flown_m,
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

/// What the second production observation records about the wake.
struct WakeObservation {
    /// The records `aiv.zrd` carries.
    records: usize,
    /// Whether the spawn held the sleeper (the measured sleep field).
    held_before: bool,
    /// Committed ticks the host was driven for before the release landed.
    ticks: u64,
    /// Decisions the law wrote during the whole observation.
    decisions: u64,
    /// How far the released record flew, in metres.
    flown_m: f32,
    /// One row per released record.
    released_rows: Vec<String>,
    /// One row per name that woke nothing, with the reason.
    refused_rows: Vec<String>,
    /// The behaviour the released record took.
    behaviour: String,
    /// Every refusal the spawn reported.
    refusals: Vec<String>,
}

/// Re-reads M01's stage and drives the composed mission headlessly — a real
/// production run over the owner's installation.
fn observe_wake_release(game_dir: &Path) -> WakeObservation {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M06").expect("the label is valid"),
        "The Red Menace",
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

    // The spawn-only frame: zero fixed ticks run, so the roster is spawned and
    // the host has consumed no directive.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();
    let spawned = app
        .world()
        .resource::<MissionActorSpawnReport>()
        .aircraft
        .iter()
        .find(|spawned| spawned.record == SLEEPER)
        .cloned()
        .expect("bsfury_1 spawns with M01's roster");
    let held_before = spawned.asleep;
    let entity = entity_named(&mut app, SLEEPER);
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

    // Drive M01's own win chain until the sleeper's own wake releases it.
    let mut ticks = 0_u64;
    while ticks < 400 {
        step_host(&mut app, win_observation(), DRIVEN_STEP);
        ticks += 1;
        if app.world().get::<PlaytestOriginalFlight>(entity).is_some() {
            break;
        }
        assert!(
            app.world().resource::<MissionHost>().terminal().is_none(),
            "the wake is on the win chain and must land before the terminal does"
        );
    }
    assert!(
        app.world().get::<PlaytestOriginalFlight>(entity).is_some(),
        "the wake released bsfury_1 within the budget"
    );

    let host = app.world().resource::<MissionHost>();
    let ledger = host.wake_enemies();
    let released_rows: Vec<String> = ledger
        .released()
        .iter()
        .map(|release| {
            format!(
                "{{\"record\": {}, \"tick\": {}}}",
                jstr(release.name()),
                release.at().0
            )
        })
        .collect();
    let refused_rows: Vec<String> = ledger
        .refused()
        .iter()
        .map(|refusal| {
            format!(
                "{{\"name\": {}, \"reason\": {}}}",
                jstr(refusal.name()),
                jstr(refusal.reason())
            )
        })
        .collect();
    let applied = ledger.applied();
    assert_eq!(applied, 1, "M01 spells exactly one WAKEUP_ENEMIES site");
    assert!(
        refused_rows.iter().any(|row| row.contains(ZEPPELIN_NAME)),
        "the non-aircraft name M01's site spells is refused by name: {refused_rows:?}"
    );
    let behaviour = format!(
        "{:?}",
        app.world()
            .get::<MissionAircraftAi>(entity)
            .map(|ai| ai.behaviour.clone())
            .unwrap_or(MissionAiBehaviour::HoldCourse)
    );

    // Fly the released record with the composed entry, one fixed tick per
    // update.
    let start = app
        .world()
        .get::<Position>(entity)
        .map(|position| position.0)
        .expect("the released record has a pose");
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let mut flown_ticks = 0_u64;
    while flown_ticks < FLY_TICKS {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        flown_ticks += 1;
    }
    let end = app
        .world()
        .get::<Position>(entity)
        .map(|position| position.0)
        .expect("the released record has a pose");
    let flown_m = start.distance(end);
    let decisions = app.world().resource::<MissionAiReport>().decisions;
    let refusals = app.world().resource::<MissionAiReport>().refusals.clone();
    assert!(
        refusals.is_empty(),
        "the AI spawn reported no refusals: {refusals:?}"
    );
    assert!(flown_m > 20.0, "the released record flew: {flown_m:.1} m");
    teardown(&mut app);

    WakeObservation {
        records: stage.aircraft.len(),
        held_before,
        ticks,
        decisions,
        flown_m,
        released_rows,
        refused_rows,
        behaviour,
        refusals,
    }
}

/// `workersvoyagezep`'s engine-part chains, in the order the record's
/// `INACTIVE<n>` stages spell them: nine right engines, nine left.
fn workers_voyage_chains() -> Vec<cs_script::ir::MemberName> {
    (1..=9)
        .flat_map(|n| {
            [
                member_chain(["workersvoyagezep", &format!("reng{n}"), "healthy"]),
                member_chain(["workersvoyagezep", &format!("leng{n}"), "healthy"]),
            ]
        })
        .collect()
}

/// The world report M01's win chain reads to reach `OBJECTIVE13`: the same
/// report `accept_m01_b_wake.rs` files.
fn win_observation() -> WorldObservation {
    let mut observation = WorldObservation::new();
    for chain in workers_voyage_chains() {
        observation = observation.member(chain, MemberObservation::no_longer_in_play([0.0; 3]));
    }
    observation
        .group(1, 0)
        .group(2, 0)
        .animation(
            "wv_drop_copilot",
            cs_script::ir::AnimationState::Running.code(),
        )
        .animation(
            "wv_pickup_copilot",
            cs_script::ir::AnimationState::Executed.code(),
        )
        .animation(
            "hooked_to_klondike",
            cs_script::ir::AnimationState::Executed.code(),
        )
}

/// One driven step of the production host, exactly `accept_m01_b_wake.rs`'s
/// seam.
fn step_host(app: &mut App, observation: WorldObservation, elapsed: Duration) {
    app.world_mut()
        .insert_resource(MissionWorldObservation { observation });
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks += 1;
    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let produced = host
        .step(app.world_mut(), elapsed)
        .expect("the driven step answers, not a refusal");
    host.settle(app.world_mut(), &produced);
    app.world_mut().insert_resource(host);
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

/// The observation as JSON: measured rows and the run's own numbers, no
/// original bytes — record names, the wake's own spellings, counts and metres
/// only.
fn render_wake_release(
    observation: &WakeObservation,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let refusals: Vec<String> = observation
        .refusals
        .iter()
        .map(|value| jstr(value))
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"zbd/c1c/m01 aiv.zrd + \
         objectives.zrd\", \"records\": {}, \"held_before\": {}, \"driven_ticks\": {}, \
         \"fly_ticks\": {}, \"decisions\": {}, \"flown_m\": {:.1}, \"behaviour\": {}, \
         \"released_rows\": [{}], \"refused_rows\": [{}], \"refusals\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.records,
        observation.held_before,
        observation.ticks,
        FLY_TICKS,
        observation.decisions,
        observation.flown_m,
        jstr(&observation.behaviour),
        observation.released_rows.join(", "),
        observation.refused_rows.join(", "),
        refusals.join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_ai_wake.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-AI-WAKE` written relative
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

/// Extracts the per-test results of the `accept_m01_play_ai_wake_` tests from
/// a recorded `cargo test` output.
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
                 \"wake-release.json\"]}}",
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
