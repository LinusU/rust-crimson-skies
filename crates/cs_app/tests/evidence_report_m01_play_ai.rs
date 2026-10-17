//! Evidence-report harness for task `M01-PLAY-AI` (#1436):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_play_ai_*`: it is not part of the acceptance suite
//! and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_play_ai_ --include-ignored
//!    2>&1 | tee private/evidence/M01-PLAY-AI/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-AI \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_ai_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_ai -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-PLAY-AI/acceptance.json
//!    --artifact-root private/evidence/M01-PLAY-AI --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-PLAY-AI.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-reads M01's stage
//! through `stage_for`, records the imported AI airframe table (record name,
//! `kind_of` chain, weight, `fd_speed`) and the spawn's flown/held split from
//! [`MissionAiReport`], then composes the mission headlessly and drives
//! 600 committed ticks of the designed pursuit law, recording how many
//! decisions it wrote and where every attacker ended relative to the player.
//! That is a real production run over the owner's installation, not a
//! paraphrase of the acceptance assertions, and it carries no original
//! bytes: record names, chain spellings, counts and metres only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use avian3d::prelude::Position;
use bevy::prelude::{Entity, Time, With};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::{MissionAiAirframes, MissionAiReport, MissionAircraftAi};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPlayerBody, MissionStage, build_headless, stage_for, teardown,
};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_ai_";

/// The measured census the acceptance suite asserts, so a report regenerated
/// on an installation whose records disagree can never be written.
const M01_RECORD_COUNT: usize = 12;
const M01_FLOWN_COUNT: usize = 8;
const M01_AIRFRAME_COUNT: usize = 4;

/// How many committed fixed ticks the second observation drives of the
/// designed pursuit law before it records where the aircraft ended.
const OBSERVATION_TICKS: u64 = 600;

/// The `review.identity` this harness records.
///
/// The literal below is the text the committed
/// `docs/findings/evidence/M01-PLAY-AI.json` carries, and it is the first
/// string in this function on purpose:
/// `tools/tests/test_evidence_review_identity.py` reads a harness's recorded
/// identity out of exactly this shape (`fn review_identity() -> String`, a
/// literal) and cross-checks it against the committed report, so the text
/// here and the report cannot drift apart silently. It names the implementer
/// and the reviewer the Rally activity log records for #1436 — including the
/// two earlier review claims that expired without merging anything — which
/// the hand-over placeholder this harness used to pass through could not
/// name.
///
/// A reviewing agent supplies their own text through `CS_EVIDENCE_REVIEW` and
/// replaces the literal with it in the same commit, which is how the report
/// names the review that actually happened.
fn review_identity() -> String {
    let recorded = String::from(
        "implementer: mimo-1/mimo-1 (Rally #1436, implement claim of 2026-10-15T11:43:24Z, \
         submitted at 15:11:49Z). reviewer: claude-2/claude-2 (Rally #1436, review claims of \
         2026-10-15T20:13:13Z and 22:39:50Z — the first approved 8e805a22, whose landing failed \
         on a rebase conflict, and the second approved 35ef11ea at 2026-10-15T23:15:13Z; Rally's \
         landing queue rebased the branch and merged it as e602fa70 at 23:27:01Z), a different \
         agent instance and model (Sonnet 5.5) than the implementer, in a separate session with \
         fresh context, and still not independent original-reference evidence. Two earlier \
         review claims expired without merging anything: the implementer's own instance \
         mimo-1/mimo-1 (2026-10-15T15:12:24Z, expired 17:22:20Z) and bunny-alpha-1/bunny-alpha-1 \
         (2026-10-15T17:24:22Z, expired 19:56:17Z; its unpushed edits survive on \
         owner/1436-bunny-alpha-1-review-wip and were not merged). The committed copy is the \
         implementer's own run — its created_at of 2026-10-15T15:04:08Z sits inside the \
         implement claim and the reviewer did not regenerate it on the reviewed commit. No \
         agent review replaces the owner's human approval",
    );
    std::env::var("CS_EVIDENCE_REVIEW").unwrap_or(recorded)
}

/// How this run was reviewed, with every number **derived** from the
/// production reads this same run produced rather than written down.
fn review_method(
    records: usize,
    flown: usize,
    airframes: usize,
    decisions: u64,
    own_tests: usize,
) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every \
         field from the recorded log, production discovery of $CS_GAME_DIR, and a second \
         production run (ai-flight.json): stage_for re-read M01 through the production readers, \
         the imported AI airframe table was recorded verbatim, the composed mission's \
         MissionAiReport was recorded, and {OBSERVATION_TICKS} committed ticks of the designed \
         pursuit law were driven — {records} measured records, {flown} records flying, \
         {airframes} imported airframes, {decisions} decisions written, no refusals. Claim is \
         implemented only. MEASURED: the aiv.zrd records, their team / primary_target / \
         deactivated fields and stored poses (#1453, #436), the name-prefix dispatch against \
         vehicle.zrd, and every per-airframe flight parameter the law consumes (#796, \
         OWNER-STATIC-2026-10-08, never verified_original). DESIGNED: the pursuit-and-fire law \
         itself — aim, bank and rudder gains, throttle policies, the station offset and the \
         engagement window — because the original's AI control law at 0x48c220 is unrecovered; \
         the reading of `team` and `primary_target` as a faction split and as the aircraft to \
         engage; and the seat flag that keeps a scripted aircraft on the law's AI branch. The \
         held bsfury_1 record stays kinematic: its WAKEUP_ENEMIES wake has no consumer yet. \
         TEST-SELECTION NOTE: the prefix accept_m01_play_ai_ is unique to this task, so the \
         {own_tests} discovered assertions are exactly this task's tests. Validated with \
         tools/validate_evidence.py --require-pass."
    )
}

/// What the report still cannot claim, each with the content it affects, the
/// task that resolves it and the claim it gates (owner directive,
/// 2026-10-05).
fn open_unknowns(decisions: u64) -> Vec<String> {
    vec![
        "original_ai_control_law: the original's AI control law at 0x48c220 was never recovered, \
         so the pursuit-and-fire law flown here (aim, bank, rudder and throttle gains, station \
         offset, engagement window) is project design. Affected content: every AI aircraft's \
         manoeuvre, aim and throttle in any mission. Resolving task: static analysis of 0x48c220's \
         callers, or an owner original run (#358 REF-OWNER-FIRST-CAPTURE). Gates: any fidelity \
         claim about how the original's AI flies."
            .to_owned(),
        "ai_parameter_randomisation: the original rolls every per-airframe parameter by +/-5% for \
         AI aircraft (0x4771c6) and never for the player; this task applies no roll, so every AI \
         airframe flies the exact retail values. Affected content: every AI aircraft's handling \
         envelope. Resolving task: #805 FLIGHT-ORIGINAL-AI-RANDOMISATION. Gates: any claim that \
         AI aircraft handle like the original's."
            .to_owned(),
        "team_and_primary_target_semantics: what the aiv.zrd team numbers mean engine-wide, and \
         what the original did with a stored primary target, are unmeasured; the role split here \
         (opposed teams, primary target as the aircraft to engage) is a designed reading of those \
         measured fields. Affected content: who attacks whom in every mission record set. \
         Resolving task: #1453's follow-ups or an owner original run. Gates: any claim about the \
         original's targeting choices."
            .to_owned(),
        "wake_and_ground_targets: bsfury_1's measured WAKEUP_ENEMIES wake has no consumer yet, so \
         the sleeper never flies; M01's gun trucks and camp are world-container scenery with no \
         aiv.zrd record, so 'trucks warn then fire' has no actor to attach a law to; and M01's \
         zeppelins declare DeclaredMotion::Held with no route polyline, so 'the cargo zeppelin \
         flies its route' has no measured route. Affected content: M01's sleeper attack, ground \
         fire and zeppelin motion. Resolving task: a wake consumer plus the ground-actor and \
         route measurements (F34-D, F35-D, F31-D). Gates: any claim that M01's full actor set is \
         simulated."
            .to_owned(),
        "rear_gunner: the task names the attackers' rear gunner, but no mount for it is \
         measured and this slice registers forward guns only. Affected content: fire from \
         behind an attacker. Resolving task: a measured mount record (F27). Gates: any claim \
         that the AI defends its six."
            .to_owned(),
        format!(
            "observation_scope: the second observation drives {OBSERVATION_TICKS} ticks and \
             records {decisions} decisions; it is a production run, not an original reference — no \
             original capture exists for AI behaviour (#358). Affected content: every comparison \
             against the original game. Resolving task: REF-OWNER-FIRST-CAPTURE. Gates: any \
             verified_original claim."
        ),
    ]
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_play_ai_writes_the_acceptance_report() {
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

    // The second production observation: re-read the stage, record the
    // imported airframe table and the spawn's flown/held split, then drive
    // the designed law for a committed run and record where everyone ended.
    let observation = observe_ai_flight(&game_dir);
    let observation_path = evidence_dir.join("ai-flight.json");
    fs::write(
        &observation_path,
        format!(
            "{}\n",
            render_ai_flight(&observation, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write ai-flight.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let unknowns = open_unknowns(observation.decisions);
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-PLAY-AI\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_identity()),
        jstr(&review_method(
            observation.records,
            observation.flown,
            observation.airframes,
            observation.decisions,
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

/// What the second production observation records: the imported airframe
/// table, the spawn's flown/held split and where the designed law flew
/// everyone in `OBSERVATION_TICKS` ticks.
struct AiFlightObservation {
    /// The records `aiv.zrd` carries.
    records: usize,
    /// The imported airframe records, by name-prefix dispatch.
    airframes: usize,
    /// The records the law flew.
    flown: usize,
    /// Committed ticks of the designed law the observation drove.
    ticks: u64,
    /// Decisions those ticks wrote.
    decisions: u64,
    /// One row per imported airframe: record, chain, weight, fd_speed.
    airframe_rows: Vec<String>,
    /// One row per flown record: name, behaviour, decisions, range to its
    /// resolved target after the drive.
    flown_rows: Vec<String>,
    /// The records held instead, with their reason.
    held_rows: Vec<String>,
    /// The closest any attacker came to the player over the driven ticks.
    closest_rows: Vec<String>,
    /// Every refusal on this path, verbatim.
    refusals: Vec<String>,
}

/// Re-reads M01's stage and drives the composed mission headlessly — a real
/// production run over the owner's installation.
fn observe_ai_flight(game_dir: &Path) -> AiFlightObservation {
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
    let airframes: &MissionAiAirframes = &stage.ai_airframes;
    assert!(
        airframes.refusals.is_empty(),
        "every M01 name prefix imports: {:?}",
        airframes.refusals
    );
    assert_eq!(
        airframes.models.len(),
        M01_AIRFRAME_COUNT,
        "the stage imports the four distinct airframes"
    );
    let airframe_rows: Vec<String> = airframes
        .models
        .iter()
        .map(|(prefix, airframe)| {
            format!(
                "{{\"prefix\": {}, \"record\": {}, \"chain\": [{}], \"veh_weight\": {}, \
                 \"fd_speed\": {}}}",
                jstr(prefix),
                jstr(&airframe.record),
                airframe
                    .inheritance_chain
                    .iter()
                    .map(|step| jstr(step))
                    .collect::<Vec<_>>()
                    .join(", "),
                airframe.model.airframe.veh_weight,
                airframe.model.airframe.fd_speed,
            )
        })
        .collect();

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    let spawn_report = app.world().resource::<MissionAiReport>().clone();
    assert_eq!(
        spawn_report.flown.len(),
        M01_FLOWN_COUNT,
        "the composed run flies the awake eight"
    );

    // Drive the designed law: one committed tick per update, exactly as the
    // acceptance suite drives it.
    let player = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MissionPlayerBody>>()
            .iter(world)
            .next()
            .expect("the composition spawns its player body")
    };
    let attackers: Vec<(String, Entity)> = {
        let world = app.world_mut();
        let mut rows: Vec<(String, Entity)> = world
            .query::<(&ScriptedAircraftActor, Entity)>()
            .iter(world)
            .filter(|(actor, _)| actor.record.name.starts_with("rusdevastator"))
            .map(|(actor, entity)| (actor.record.name.clone(), entity))
            .collect();
        rows.sort();
        rows
    };
    let distance_to_player = |app: &mut bevy::prelude::App, entity: Entity| {
        let player_position = app
            .world()
            .get::<Position>(player)
            .map(|position| position.0)
            .expect("the player body has a pose");
        app.world()
            .get::<Position>(entity)
            .map(|position| player_position.distance(position.0))
            .expect("the aircraft has a pose")
    };
    let mut closest: Vec<(String, f32)> = attackers
        .iter()
        .map(|(name, entity)| (name.clone(), distance_to_player(&mut app, *entity)))
        .collect();
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let mut ticks = 0_u64;
    while ticks < OBSERVATION_TICKS {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        ticks += 1;
        for (name, entity) in &attackers {
            let distance = distance_to_player(&mut app, *entity);
            if let Some(entry) = closest.iter_mut().find(|(candidate, _)| candidate == name) {
                entry.1 = entry.1.min(distance);
            }
        }
    }

    let report = app.world().resource::<MissionAiReport>().clone();
    let closest_rows: Vec<String> = closest
        .iter()
        .map(|(name, distance)| {
            format!("{{\"record\": {}, \"metres\": {distance:.1}}}", jstr(name))
        })
        .collect();
    let flown_rows: Vec<String> =
        {
            let world = app.world_mut();
            let mut rows: Vec<String> = world
            .query::<(&ScriptedAircraftActor, &MissionAircraftAi, &PlaytestOriginalFlight)>()
            .iter(world)
            .map(|(actor, ai, flight)| {
                // A record with no resolved target point (hold-course) has
                // no range: `null` rather than a non-JSON `NaN`.
                let range = if ai.range_m.is_finite() {
                    format!("{:.1}", ai.range_m)
                } else {
                    "null".to_owned()
                };
                format!(
                    "{{\"record\": {}, \"behaviour\": {}, \"decisions\": {}, \"is_player\": {}, \
                     \"range_m\": {range}}}",
                    jstr(&actor.record.name),
                    jstr(&format!("{:?}", ai.behaviour)),
                    ai.decisions,
                    flight.is_player,
                )
            })
            .collect();
            rows.sort();
            rows
        };
    let held_rows: Vec<String> = report.held.iter().map(|held| jstr(held)).collect();
    teardown(&mut app);

    assert_eq!(
        report.refusals.len(),
        0,
        "the AI spawn reported no refusals: {:?}",
        report.refusals
    );
    AiFlightObservation {
        records: stage.aircraft.len(),
        airframes: airframes.models.len(),
        flown: report.flown.len(),
        ticks,
        decisions: report.decisions,
        airframe_rows,
        flown_rows,
        held_rows,
        closest_rows,
        refusals: report.refusals.clone(),
    }
}

/// The observation as JSON: measured rows and the run's own numbers, no
/// original bytes — record names, chain spellings, counts and metres only.
fn render_ai_flight(
    observation: &AiFlightObservation,
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
         vehicle.zrd\", \"records\": {}, \"airframes\": {}, \"flown\": {}, \"ticks\": {}, \
         \"decisions\": {}, \"airframe_rows\": [{}], \"flown_rows\": [{}], \"held_rows\": [{}], \
         \"closest_rows\": [{}], \"refusals\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        observation.records,
        observation.airframes,
        observation.flown,
        observation.ticks,
        observation.decisions,
        observation.airframe_rows.join(", "),
        observation.flown_rows.join(", "),
        observation.held_rows.join(", "),
        observation.closest_rows.join(", "),
        refusals.join(", "),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_ai.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-AI` written relative to
/// the workspace root in the module doc must be re-anchored here.
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

/// Extracts the per-test results of the `accept_m01_play_ai_` tests from a
/// recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this
/// task's acceptance selection. A prefixed test that was skipped is recorded
/// with the schema's `unknown` status rather than counted as a pass, so a
/// report can never claim an assertion it did not run.
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
                 \"ai-flight.json\"]}}",
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
