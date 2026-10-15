//! Evidence-report harness for task #1486 `M01-PLAY-TRIGGERS-WINDOWED`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//!
//! Not named `accept_m01_play_triggers_windowed_*`: it is not part of the
//! acceptance suite and it fails loudly when its inputs are missing, so the
//! task's own test selection can never pick it up as an acceptance test. It is
//! the **windowed half** of #1439: the headless proof that M01's proximity
//! trigger fires from the player's real position lives in
//! `tests/campaign/m01_play_triggers_observe.rs`
//! (`accept_m01_play_triggers_observe_the_trigger_fires_from_the_players_real_position`),
//! and this harness drives the same trigger in the **windowed** composition —
//! a real window, a real renderer, a real framebuffer.
//!
//! Run it from the workspace root, after the acceptance suite and after
//! building the windowed binary, exactly as:
//!
//! 1. ```sh
//!    cargo build --release --locked -p cs_app --bin cs
//!    mkdir -p private/evidence/M01-PLAY-TRIGGERS-WINDOWED
//!    cargo test --workspace --locked -- accept_m01_play_triggers_windowed_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-WINDOWED/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-WINDOWED \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_windowed_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!    CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" \
//!    CS_CAPABILITIES="$CS_CAPABILITIES" \
//!      cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_windowed -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS-WINDOWED/acceptance.json \
//!      --artifact-root private/evidence/M01-PLAY-TRIGGERS-WINDOWED --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/M01-PLAY-TRIGGERS-WINDOWED.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the **windowed run's
//! own trace** (`capture/trace.jsonl`, written by `cs --mission M01` under
//! `CS_MISSION_CAPTURE=1`) and the framebuffer PNGs that run delivered.
//! Nothing is typed in by hand.
//!
//! # What the windowed run is asked to do, and why
//!
//! M01 starts 5 311 m from `workersvoyagezep` against the proximity trigger's
//! 700 m radius, and flying the authored heading never passes inside about
//! 1 400 m, so the trigger cannot fire from the measured start by simply
//! letting the run fly (measured 2026-10-15, task #1480). A legitimate
//! placement is therefore derived here **from the live composition**, never
//! hard-coded: the player is put `APPROACH_M` metres from the anchor on the
//! line toward the measured start (at the measured start's own altitude),
//! heading at the anchor, and the run waits for block index 2 to complete. The
//! placement and the wait travel to the run through the scenario variables
//! `CS_MISSION_START_M`/`CS_MISSION_START_HEADING_RAD`/`CS_MISSION_WATCH_BLOCK`/
//! `CS_MISSION_WATCH_MAX_FRAMES` (`mission_session::scenario`); the run refuses
//! a half-stated scenario and refuses to end unfired.
//!
//! The placement is a **test placement**, never the measured start: the trace
//! carries it as a `scenario` record and the report lists it under
//! `overrides`. The player is still the production body under the production
//! flight law, observed by the production world observer, and the block must
//! complete from where that body really is.
//!
//! The claim is `implemented`, never `checked` or `verified_original`: the
//! windowed run is this machine's own frames and no original executable ran.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::{App, Transform};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionStage, PLAYER_MEMBER, build_headless, stage_for, subject_node_name, teardown,
};
use cs_app::world_actors::WorldActorBinding;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_play_triggers_windowed_";
/// The task identity this report is written for.
const TASK_ID: &str = "M01-PLAY-TRIGGERS-WINDOWED";
/// The one mission this task runs.
const MISSION: &str = "M01";
/// The work order's own discovery title (`mission_launch` binds the identity
/// with it).
const MISSION_TITLE: &str = "The Lost Treasure";
/// The measured subject name M01's `TRAVELERS` anchor spells.
const ANCHOR: &str = "workersvoyagezep";
/// `OBJECTIVE3`'s zero-based block index — the one proximity trigger.
const PROXIMITY_BLOCK: u32 = 2;
/// The record's proximity radius, in metres.
const RADIUS_M: f64 = 700.0;
/// How far from the anchor the derived test placement starts, in metres. It is
/// outside [`RADIUS_M`], so the trigger can only fire after the body has really
/// flown in; it is short enough that the straight, level approach reaches the
/// radius well inside [`WATCH_MAX_FRAMES`].
const APPROACH_M: f64 = 1300.0;
/// The rendered frames the wait for block 2 may last. The approach is about
/// 600 m at the measured flight law's own speed, so this is a generous budget
/// rather than a measurement.
const WATCH_MAX_FRAMES: u32 = 6000;
/// The record's own completion effects on `OBJECTIVE3`, in the order
/// `scenario::watch_trigger` writes them: the `NAP` target, the two `WAKE`
/// targets and the two `KILL` targets (record `4`, `27`/`51`, `41`/`48` →
/// indexes 3, 26/50, 40/47). A killed objective projects **no row**, which the
/// trace writes as `null`.
const TRIGGER_EFFECTS: [(&str, u32, JsonState); 5] = [
    ("nap", 3, JsonState::Named("Napping")),
    ("wake", 26, JsonState::Named("Awake")),
    ("wake", 50, JsonState::Named("Awake")),
    ("kill", 40, JsonState::Absent),
    ("kill", 47, JsonState::Absent),
];

/// One expected effect state: a named lifecycle, or no row at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonState {
    /// The lifecycle's `Debug` name, as the trace writes it.
    Named(&'static str),
    /// `null`: the objective projects no row (it was removed).
    Absent,
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE, CS_CAPABILITIES"]
fn evidence_report_m01_play_triggers_windowed_writes_the_acceptance_report() {
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
    let engine_image = env_var("CS_ENGINE_IMAGE");
    let capabilities = env_var("CS_CAPABILITIES");
    for capability in ["retail", "gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|declared| declared.trim() == capability),
            "this harness claims `{capability}`: CS_CAPABILITIES must list it, found \
             {capabilities:?}"
        );
    }

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    // The windowed mission binary is resolved **before** the slow placement
    // derivation, so a missing binary is a loud, quick refusal rather than a
    // minute-long wait that ends in a panic.
    let bin = mission_bin();

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );
    assert!(
        suite
            .assertions
            .iter()
            .any(|(id, status)| id.contains("needs_a_capture") && *status == "pass"),
        "the run that keeps an ordinary mission free of a scenario must be in the log: {:?}",
        suite.assertions
    );

    // The placement, derived from the live composition rather than written
    // down: the anchor the record's condition reads, and the measured start.
    let placement = derive_placement(&game_dir);
    assert!(
        placement.start_distance_m > RADIUS_M,
        "the derived placement must be outside the {RADIUS_M} m radius, so the body has to fly \
         in: {:?}",
        placement
    );

    // The windowed run itself: the production binary, `--mission M01`, a real
    // window, the scenario in its environment. This is the artifact that makes
    // the report a windowed claim.
    let capture_dir = evidence_dir.join("capture");
    let capture = run_capture(
        &evidence_dir,
        &capture_dir,
        &bin,
        &game_dir,
        &engine_image,
        &capabilities,
        &placement,
    );
    assert_eq!(
        capture.exit_code, 0,
        "the windowed run must exit 0; its log:\n{}",
        capture.log
    );

    // What the run's own trace says: a `scenario` record naming the placement,
    // the `trigger` record for block 2, the observed player's real position
    // inside the radius, and the record's own effects.
    let trace_path = capture_dir.join("trace.jsonl");
    let trace = fs::read_to_string(&trace_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", trace_path.display()));
    let records = trace_records(&trace);
    let trigger = windowed_trigger(&records, &placement);
    assert_trigger_effects(&trigger);
    let trigger_png = capture_dir.join(format!("frame_{:05}_trigger.png", trigger.frame));
    assert!(
        trigger_png.is_file(),
        "the run must leave the trigger frame's own framebuffer PNG at {}",
        trigger_png.display()
    );
    assert!(
        fs::metadata(&trigger_png)
            .map(|meta| meta.len())
            .unwrap_or(0)
            > 0,
        "the trigger frame's PNG is empty: {}",
        trigger_png.display()
    );

    // The audio half of the capture: the launch sound's own source boundary,
    // written by the run only when the machine's audio backend really pulled
    // audible samples. The report claims `audio`, so the record is checked
    // and listed as an artifact — it is a source pull, never a speaker
    // capture, which stays the owner's `human_review`.
    let playback_path = capture_dir.join("playback.json");
    let playback = fs::read_to_string(&playback_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", playback_path.display()));
    let playback = parse_json(playback.trim())
        .map(|(value, _)| value)
        .unwrap_or_else(|| panic!("{} is not JSON", playback_path.display()));
    assert_eq!(
        playback.get_str("boundary"),
        Some("sample_probe_source_pull"),
        "the playback record must be the source boundary it claims: {playback:?}"
    );
    assert_eq!(
        playback.get_str("backend"),
        Some("audible"),
        "the report claims `audio`, so the launch sound must have been played by an audible \
         backend: {playback:?}"
    );
    assert!(
        playback.get_number("sample_values_pulled").unwrap_or(0.0) > 0.0,
        "the output stream must have pulled samples: {playback:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The run's own summary artifact: placement, argv, exit code, trigger,
    // effects and the observed approach — derived, never typed in.
    let run_path = evidence_dir.join("windowed-run.json");
    fs::write(
        &run_path,
        format!(
            "{}\n",
            render_windowed_run(
                &placement,
                &capture,
                &trigger,
                &install_sha256,
                &candidate_tree
            )
        ),
    )
    .expect("write windowed-run.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&capture.log_path, "log", &evidence_dir),
        artifact(&trace_path, "jsonl", &evidence_dir),
        artifact(&run_path, "json", &evidence_dir),
        artifact(&trigger_png, "png", &evidence_dir),
        artifact(&playback_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let overrides = vec![
        format!(
            "{}={}",
            cs_app::mission_session::START_ENV,
            placement.start_text()
        ),
        format!(
            "{}={}",
            cs_app::mission_session::START_HEADING_ENV,
            placement.heading_text()
        ),
        format!(
            "{}={}",
            cs_app::mission_session::WATCH_BLOCK_ENV,
            PROXIMITY_BLOCK
        ),
        format!(
            "{}={}",
            cs_app::mission_session::WATCH_MAX_FRAMES_ENV,
            WATCH_MAX_FRAMES
        ),
    ];
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": {},\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": {},\n \
         \"capabilities\": [\"retail\", \"gpu\", \"audio\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": \
         [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \
         \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(TASK_ID),
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        trigger.tick,
        str_array(&overrides),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(&review_method(&placement, &trigger, &capture)),
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

/// How this run was reviewed.
///
/// The prose names the windowed run and its own limits, so a regenerated
/// report cannot describe more than this one observed.
fn review_method(placement: &Placement, trigger: &TriggerRecord, capture: &CaptureRun) -> String {
    format!(
        "Acceptance suite run locally with `retail` (CS_GAME_DIR) and this task's own windowed \
         run executed on this machine's GPU (`gpu`) with the machine's audio backend (`audio`): \
         the harness ran `cs --cs-path $CS_GAME_DIR --mission M01` with CS_MISSION_CAPTURE=1 and \
         the derived scenario (exit {}), which opened a real window, drew the original M01 world \
         with the renderer, and delivered framebuffer PNGs, the run trace and the launch sound's \
         own source-boundary playback record. Every field in this report is derived from the \
         recorded log, production discovery of $CS_GAME_DIR, and that run's own trace.jsonl and \
         playback.json. MEASURED: the anchor {} and the measured start are read from the \
         live composition (stage_for + a headless composition of the same records the windowed \
         run loads); the placement is the measured start's altitude {} m and {} m from the anchor \
         along the line toward the measured start, heading at it; the block index {} and its \
         record's effect lists (NAP 4, WAKE 27/51, KILL 41/48) come from the recovered engine \
         model. The trigger completed on tick {} (frame {}): the observed player row was {:.1} m from \
         the observed anchor row (radius {} m) and the effect states were {:?}; the framebuffer \
         PNG of that frame is an artifact. DESIGNED: the placement itself is a test placement, \
         never the measured start, and is listed under overrides; which pose source an observer \
         reads is this reimplementation's own contract. {}. LIMITS: no original executable ran, \
         so nothing here is verified_original; the flight law is the statically recovered \
         fixed-wing law, still uncalibrated against an original run (#358); the windowed \
         criterion this task carries is met by the run recorded here and by no claim about how \
         the original behaved. Validated with tools/validate_evidence.py --require-pass.",
        capture.exit_code,
        placement.anchor_text(),
        placement.measured_start_m[1],
        APPROACH_M.round(),
        PROXIMITY_BLOCK,
        trigger.tick,
        trigger.frame,
        trigger.distance_m,
        RADIUS_M,
        trigger.effect_states(),
        placement.note(),
    )
}

// ------------------------------------------------------------- placement ---

/// The derived test placement, plus the live-composition facts it came from.
#[derive(Clone, Debug)]
struct Placement {
    /// Where the player is put, metres.
    start_m: [f64; 3],
    /// Heading, radians.
    heading_rad: f64,
    /// The anchor's live position, metres.
    anchor_m: [f64; 3],
    /// The measured start pose, metres.
    measured_start_m: [f64; 3],
    /// The measured start's heading, radians.
    measured_heading_rad: f64,
    /// The distance from [`Self::start_m`] to [`Self::anchor_m`].
    start_distance_m: f64,
}

impl Placement {
    /// The `CS_MISSION_START_M` text.
    fn start_text(&self) -> String {
        format!(
            "{},{},{}",
            self.start_m[0], self.start_m[1], self.start_m[2]
        )
    }

    /// The `CS_MISSION_START_HEADING_RAD` text.
    fn heading_text(&self) -> String {
        self.heading_rad.to_string()
    }

    /// The anchor as a readable triple.
    fn anchor_text(&self) -> String {
        format!(
            "[{:.1}, {:.1}, {:.1}]",
            self.anchor_m[0], self.anchor_m[1], self.anchor_m[2]
        )
    }

    /// The measured start as a readable triple.
    fn measured_text(&self) -> String {
        format!(
            "[{:.1}, {:.1}, {:.1}]",
            self.measured_start_m[0], self.measured_start_m[1], self.measured_start_m[2]
        )
    }

    /// How the placement was derived, for the report's prose.
    fn note(&self) -> String {
        format!(
            "The placement is derived from the live composition: the anchor sits at {}, the \
             measured start at {} (heading {:.4} rad) {:.0} m away; the run starts {:.1} m from the \
             anchor on that line, at the measured start's own altitude, heading at the anchor",
            self.anchor_text(),
            self.measured_text(),
            self.measured_heading_rad,
            self.measured_start_m
                .iter()
                .zip(self.anchor_m)
                .map(|(start, anchor)| (start - anchor).powi(2))
                .sum::<f64>()
                .sqrt(),
            self.start_distance_m,
        )
    }
}

/// Derives the windowed placement from the live composition: M01's stage over
/// the owner's installation, composed headlessly, then the anchor's live pose
/// (the same `WorldActorBinding` `Transform` the world observer reports) and
/// the stage's measured start pose.
fn derive_placement(game_dir: &Path) -> Placement {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new(MISSION).expect("M01 is a valid work-order label"),
        MISSION_TITLE,
    )
    .expect("M01's launch closure plans");
    let stage: MissionStage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    let measured = stage.start.position;
    let measured_heading = stage.start.heading;

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    for _ in 0..2 {
        app.update();
    }
    let anchor = anchor_position(&mut app);
    teardown(&mut app);

    let anchor_m = [
        f64::from(anchor[0]),
        f64::from(anchor[1]),
        f64::from(anchor[2]),
    ];
    let measured_m = [
        f64::from(measured[0]),
        f64::from(measured[1]),
        f64::from(measured[2]),
    ];
    let dx = measured_m[0] - anchor_m[0];
    let dz = measured_m[2] - anchor_m[2];
    let horizontal = (dx * dx + dz * dz).sqrt();
    assert!(
        horizontal > RADIUS_M,
        "the measured start must be horizontally outside the radius for the line toward it to be \
         meaningful: {horizontal} m"
    );
    let start_m = [
        anchor_m[0] + dx / horizontal * APPROACH_M,
        measured_m[1],
        anchor_m[2] + dz / horizontal * APPROACH_M,
    ];
    // `-Z` nose under `Ry`: forward = (-sin h, 0, -cos h), so the heading that
    // flies at the anchor is atan2(-dx, -dz) of (anchor - start).
    let to_anchor = [anchor_m[0] - start_m[0], anchor_m[2] - start_m[2]];
    let heading = (-to_anchor[0]).atan2(-to_anchor[1]);
    let start_distance_m = {
        let dx = start_m[0] - anchor_m[0];
        let dy = start_m[1] - anchor_m[1];
        let dz = start_m[2] - anchor_m[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    Placement {
        start_m,
        heading_rad: heading,
        anchor_m,
        measured_start_m: measured_m,
        measured_heading_rad: f64::from(measured_heading),
        start_distance_m,
    }
}

/// The anchor binding's live pose, exactly as `mission_world_observation`
/// reads it.
fn anchor_position(app: &mut App) -> [f32; 3] {
    let world = app.world_mut();
    let mut query = world.query::<(&WorldActorBinding, &Transform)>();
    query
        .iter(world)
        .find(|(binding, _)| subject_node_name(&binding.subject) == Some(ANCHOR))
        .map(|(_, transform)| transform.translation.to_array())
        .unwrap_or_else(|| panic!("the composed M01 world binds the anchor `{ANCHOR}`"))
}

// --------------------------------------------------------- windowed run ---

/// What the windowed run's process left behind.
struct CaptureRun {
    /// The argv the run was given, as text.
    argv: Vec<String>,
    /// Its exit status.
    exit_code: i32,
    /// Its combined stdout and stderr.
    log: String,
    /// Where that log was written.
    log_path: PathBuf,
}

/// Runs `cs --cs-path <dir> --mission M01` with the capture and scenario in
/// its environment, from the workspace root, and records its output.
fn run_capture(
    evidence_dir: &Path,
    capture_dir: &Path,
    bin: &Path,
    game_dir: &Path,
    engine_image: &str,
    capabilities: &str,
    placement: &Placement,
) -> CaptureRun {
    let argv = vec![
        bin.display().to_string(),
        "--cs-path".to_owned(),
        game_dir.display().to_string(),
        "--mission".to_owned(),
        MISSION.to_owned(),
    ];
    let output = Command::new(bin)
        .args(&argv[1..])
        .current_dir(Path::new(&git(&["rev-parse", "--show-toplevel"])))
        .env("CS_MISSION_CAPTURE", "1")
        .env("CS_EVIDENCE_DIR", capture_dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .env(cs_app::mission_session::START_ENV, placement.start_text())
        .env(
            cs_app::mission_session::START_HEADING_ENV,
            placement.heading_text(),
        )
        .env(
            cs_app::mission_session::WATCH_BLOCK_ENV,
            PROXIMITY_BLOCK.to_string(),
        )
        .env(
            cs_app::mission_session::WATCH_MAX_FRAMES_ENV,
            WATCH_MAX_FRAMES.to_string(),
        )
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "cannot run the windowed mission binary {}: {error}",
                bin.display()
            )
        });
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log_path = evidence_dir.join("capture.log");
    fs::write(&log_path, &log)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", log_path.display()));
    let exit_code = output.status.code().unwrap_or(-1);
    assert!(
        output.status.code().is_some(),
        "the windowed run must exit by itself, never on a signal: {}",
        output.status
    );
    CaptureRun {
        argv,
        exit_code,
        log,
        log_path,
    }
}

/// The windowed mission binary this harness drives. `CS_MISSION_BIN` names it;
/// without that, the release binary of this checkout is used. Either way the
/// path must exist: a missing binary is a loud refusal, never a silent skip.
fn mission_bin() -> PathBuf {
    if let Ok(named) = std::env::var("CS_MISSION_BIN") {
        let path = PathBuf::from(&named);
        assert!(
            path.is_file(),
            "CS_MISSION_BIN names {named}, which is not a file"
        );
        return path;
    }
    let path = Path::new(&git(&["rev-parse", "--show-toplevel"])).join("target/release/cs");
    assert!(
        path.is_file(),
        "the windowed mission binary {} does not exist: build it with `cargo build --release \
         --locked -p cs_app --bin cs`, or name another with CS_MISSION_BIN",
        path.display()
    );
    path
}

// ------------------------------------------------------- the run's trace ---

/// The trigger record, read out of the run's own trace.
struct TriggerRecord {
    /// The fixed tick it completed on.
    tick: u64,
    /// The rendered frame the completion was read on.
    frame: u32,
    /// The observed player's position when it completed.
    player_m: [f64; 3],
    /// The observed player's distance from the observed anchor.
    distance_m: f64,
    /// The record's own effect rows: `(role, target, state)`.
    effects: Vec<(String, u32, Option<String>)>,
}

impl TriggerRecord {
    /// Every effect state as a readable list.
    fn effect_states(&self) -> Vec<String> {
        self.effects
            .iter()
            .map(|(role, target, state)| {
                format!("{role} {target} {}", state.as_deref().unwrap_or("null"))
            })
            .collect()
    }
}

/// Finds the trace's `trigger` record and asserts it names block 2 completing
/// from the player's real position, inside the radius, after an approach.
fn windowed_trigger(records: &[Json], placement: &Placement) -> TriggerRecord {
    let scenario = records
        .iter()
        .find(|record| record.get_str("kind") == Some("scenario"))
        .unwrap_or_else(|| panic!("the run's trace carries no scenario record: {:?}", records));
    let start = scenario
        .get("start_override")
        .unwrap_or_else(|| panic!("the scenario record names no start override: {scenario:?}"));
    let position = start
        .get("position_m")
        .and_then(Json::as_numbers)
        .unwrap_or_else(|| panic!("the scenario's start override names no position: {start:?}"));
    assert_eq!(
        position.len(),
        3,
        "the start override must be x,y,z: {position:?}"
    );
    let scenario_distance = {
        let dx = position[0] - placement.anchor_m[0];
        let dy = position[1] - placement.anchor_m[1];
        let dz = position[2] - placement.anchor_m[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    assert!(
        (scenario_distance - placement.start_distance_m).abs() < 1.0,
        "the run's own scenario record must name the derived placement ({scenario_distance} vs \
         {} m): {scenario:?}",
        placement.start_distance_m
    );
    assert!(
        (start.get_number("heading_rad").unwrap_or(f64::NAN) - placement.heading_rad).abs()
            < 1.0e-3,
        "the run's scenario record must name the derived heading: {scenario:?}"
    );

    let trigger = records
        .iter()
        .find(|record| record.get_str("kind") == Some("trigger"))
        .unwrap_or_else(|| {
            panic!("the windowed run never completed a block; its trace: {records:?}")
        });
    let block = u32::try_from(trigger.get_number("block").unwrap_or(-1.0) as i64)
        .expect("the trigger record's block is a non-negative index");
    assert_eq!(
        block, PROXIMITY_BLOCK,
        "the windowed run's trigger is the one proximity block `OBJECTIVE3`: {trigger:?}"
    );
    let tick = trigger.get_number("tick").unwrap_or(-1.0) as u64;
    let frame = trigger.get_number("frame").unwrap_or(-1.0) as u32;
    assert!(
        frame > 1,
        "the trigger was read on a real frame: {trigger:?}"
    );

    let members = trigger
        .get("members")
        .and_then(Json::as_arr)
        .unwrap_or_else(|| panic!("the trigger record carries no member rows: {trigger:?}"));
    let player = member_position(members, PLAYER_MEMBER)
        .unwrap_or_else(|| panic!("the trigger record carries no player row: {trigger:?}"));
    let anchor = member_position(members, ANCHOR)
        .unwrap_or_else(|| panic!("the trigger record carries no anchor row: {trigger:?}"));
    let distance_m = {
        let dx = player[0] - anchor[0];
        let dy = player[1] - anchor[1];
        let dz = player[2] - anchor[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    assert!(
        distance_m < RADIUS_M,
        "the observed player must be inside the {RADIUS_M} m radius when the block completes, \
         found {distance_m} m: {trigger:?}"
    );
    assert!(
        (anchor[0] - placement.anchor_m[0]).abs() < 1.0
            && (anchor[2] - placement.anchor_m[2]).abs() < 1.0,
        "the anchor the run observed must be the anchor the placement was derived from: \
         {anchor:?} vs {:?}",
        placement.anchor_m
    );

    // The body really flew in: the run's first sampled tick is outside the
    // radius and well behind the trigger's own position.
    let first_tick = records
        .iter()
        .find(|record| record.get_str("kind") == Some("tick"))
        .and_then(|record| record.get("position_m").and_then(Json::as_numbers))
        .unwrap_or_else(|| panic!("the run's trace carries no tick sample: {records:?}"));
    let first_distance = {
        let dx = first_tick[0] - anchor[0];
        let dy = first_tick[1] - anchor[1];
        let dz = first_tick[2] - anchor[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    assert!(
        first_distance > RADIUS_M,
        "the run's first sampled tick must start outside the radius ({first_distance} m): \
         {records:?}"
    );
    let moved = {
        let dx = player[0] - first_tick[0];
        let dy = player[1] - first_tick[1];
        let dz = player[2] - first_tick[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    assert!(
        moved > RADIUS_M * 0.5,
        "the player must have flown a real approach between the first sampled tick and the \
         trigger ({moved} m): {records:?}"
    );

    let effects = trigger
        .get("effects")
        .and_then(Json::as_arr)
        .unwrap_or_else(|| panic!("the trigger record carries no effect rows: {trigger:?}"))
        .iter()
        .map(|row| {
            let role = row
                .get_str("role")
                .unwrap_or_else(|| panic!("an effect row names no role: {row:?}"))
                .to_owned();
            let target = row.get_number("target").unwrap_or(-1.0) as u32;
            let state = match row.get("state") {
                Some(Json::Null) | None => None,
                Some(Json::Str(name)) => Some(name.clone()),
                other => panic!("an effect row's state is neither a name nor null: {other:?}"),
            };
            (role, target, state)
        })
        .collect();
    TriggerRecord {
        tick,
        frame,
        player_m: [player[0], player[1], player[2]],
        distance_m,
        effects,
    }
}

/// Asserts the trigger's effect rows are exactly the record's own list, in its
/// own order, with the states the recovered model produced.
fn assert_trigger_effects(trigger: &TriggerRecord) {
    let expected: Vec<(&str, u32, JsonState)> = TRIGGER_EFFECTS.to_vec();
    assert_eq!(
        trigger.effects.len(),
        expected.len(),
        "the trigger must report the record's own effect targets: {:?}",
        trigger.effects
    );
    for ((role, target, state), (want_role, want_target, want_state)) in
        trigger.effects.iter().zip(expected)
    {
        assert_eq!(role, want_role, "effect role: {:?}", trigger.effects);
        assert_eq!(*target, want_target, "effect target: {:?}", trigger.effects);
        match want_state {
            JsonState::Named(name) => assert_eq!(
                state.as_deref(),
                Some(name),
                "effect {want_role} {want_target} must be {name}: {:?}",
                trigger.effects
            ),
            JsonState::Absent => assert!(
                state.is_none(),
                "effect {want_role} {want_target} must project no row: {:?}",
                trigger.effects
            ),
        }
    }
}

/// One member row's observed position, by its exact chain spelling.
fn member_position(members: &[Json], name: &str) -> Option<[f64; 3]> {
    members.iter().find_map(|row| {
        if row.get_str("member") != Some(name) {
            return None;
        }
        let position = row.get("position_m").and_then(Json::as_numbers)?;
        let position: [f64; 3] = position.try_into().ok()?;
        Some(position)
    })
}

/// Every JSON object of a JSON-Lines trace, in order.
fn trace_records(trace: &str) -> Vec<Json> {
    trace
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            parse_json(line.trim())
                .map(|(value, _)| value)
                .unwrap_or_else(|| panic!("the trace holds a line that is not JSON: {line:?}"))
        })
        .collect()
}

/// The run's own summary: what was asked for, what it did, what it observed.
fn render_windowed_run(
    placement: &Placement,
    capture: &CaptureRun,
    trigger: &TriggerRecord,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"argv\": {}, \"exit_code\": {}, \
         \"placement\": {{\"start_m\": [{}, {}, {}], \"heading_rad\": {}, \"anchor_m\": [{}, {}, \
         {}], \"measured_start_m\": [{}, {}, {}], \"start_distance_m\": {}}}, \"block\": {}, \
         \"trigger\": {{\"tick\": {}, \"frame\": {}, \"player_m\": [{}, {}, {}], \
         \"distance_m\": {}, \"effects\": [{}]}}}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        str_array(&capture.argv),
        capture.exit_code,
        placement.start_m[0],
        placement.start_m[1],
        placement.start_m[2],
        placement.heading_rad,
        placement.anchor_m[0],
        placement.anchor_m[1],
        placement.anchor_m[2],
        placement.measured_start_m[0],
        placement.measured_start_m[1],
        placement.measured_start_m[2],
        placement.start_distance_m,
        PROXIMITY_BLOCK,
        trigger.tick,
        trigger.frame,
        trigger.player_m[0],
        trigger.player_m[1],
        trigger.player_m[2],
        trigger.distance_m,
        trigger
            .effects
            .iter()
            .map(|(role, target, state)| format!(
                "{{\"role\": {}, \"target\": {target}, \"state\": {}}}",
                jstr(role),
                state.as_deref().map_or_else(|| "null".to_owned(), jstr)
            ))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

// ------------------------------------------------------------ tiny JSON ---

/// Just enough JSON to read the run's own trace: objects keep their key order,
/// numbers are `f64`, and nothing here is ever written back out.
#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool,
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Self::Arr(items) => Some(items),
            _ => None,
        }
    }

    fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Self::Str(text)) => Some(text),
            _ => None,
        }
    }

    fn get_number(&self, key: &str) -> Option<f64> {
        match self.get(key) {
            Some(Self::Num(value)) => Some(*value),
            _ => None,
        }
    }

    fn as_numbers(&self) -> Option<Vec<f64>> {
        self.as_arr()?
            .iter()
            .map(|item| match item {
                Self::Num(value) => Some(*value),
                _ => None,
            })
            .collect()
    }
}

/// Parses one JSON value, returning it and the rest of the input.
fn parse_json(text: &str) -> Option<(Json, &str)> {
    let text = text.trim_start();
    let mut chars = text.chars();
    match chars.next()? {
        '{' => {
            let mut entries = Vec::new();
            let mut rest = chars.as_str().trim_start();
            if let Some(after) = rest.strip_prefix('}') {
                return Some((Json::Obj(entries), after));
            }
            loop {
                let (key, after) = parse_string(rest)?;
                let after = after.trim_start().strip_prefix(':')?.trim_start();
                let (value, after) = parse_json(after)?;
                entries.push((key, value));
                let after = after.trim_start();
                if let Some(after) = after.strip_prefix('}') {
                    return Some((Json::Obj(entries), after));
                }
                rest = after.strip_prefix(',')?.trim_start();
            }
        }
        '[' => {
            let mut items = Vec::new();
            let mut rest = chars.as_str().trim_start();
            if let Some(after) = rest.strip_prefix(']') {
                return Some((Json::Arr(items), after));
            }
            loop {
                let (value, after) = parse_json(rest)?;
                items.push(value);
                let after = after.trim_start();
                if let Some(after) = after.strip_prefix(']') {
                    return Some((Json::Arr(items), after));
                }
                rest = after.strip_prefix(',')?.trim_start();
            }
        }
        '"' => {
            let (value, rest) = parse_string(text)?;
            Some((Json::Str(value), rest))
        }
        't' => text.strip_prefix("true").map(|rest| (Json::Bool, rest)),
        'f' => text.strip_prefix("false").map(|rest| (Json::Bool, rest)),
        'n' => text.strip_prefix("null").map(|rest| (Json::Null, rest)),
        _ => {
            let end = text
                .find(|character: char| {
                    !matches!(character, '0'..='9' | '-' | '+' | '.' | 'e' | 'E')
                })
                .unwrap_or(text.len());
            let (number, rest) = text.split_at(end);
            number
                .parse::<f64>()
                .ok()
                .map(|value| (Json::Num(value), rest))
        }
    }
}

/// Parses a JSON string, returning it unescaped and the rest of the input.
fn parse_string(text: &str) -> Option<(String, &str)> {
    let mut rest = text.strip_prefix('"')?;
    let mut out = String::new();
    loop {
        let mut chars = rest.char_indices();
        let (index, character) = chars.next()?;
        match character {
            '"' => return Some((out, &rest[index + 1..])),
            '\\' => {
                let (escape_index, escape) = chars.next()?;
                let after = &rest[escape_index + escape.len_utf8()..];
                match escape {
                    '"' | '\\' | '/' => out.push(escape),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let digits = after.get(0..4)?;
                        let code = u32::from_str_radix(digits, 16).ok()?;
                        out.push(char::from_u32(code)?);
                        rest = &after[4..];
                        continue;
                    }
                    other => panic!("the trace holds an unknown JSON escape `\\{other}`"),
                }
                rest = after;
            }
            other => {
                out.push(other);
                rest = &rest[index + other.len_utf8()..];
            }
        }
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_triggers_windowed.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-TRIGGERS-WINDOWED` written
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the `accept_m01_play_triggers_windowed_`
/// tests from a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, never from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this task's
/// selection. A prefixed test that was skipped is recorded with the schema's
/// `unknown` status rather than counted as a pass, so the report can never
/// claim an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            record(&mut suite, name, status);
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        if !carries_prefix(name) {
            continue;
        }
        match finished(tail.trim()) {
            Some(status) => record(&mut suite, name.to_owned(), status),
            None => pending.push_back(name.to_owned()),
        }
    }
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests, matched on the name's
/// last path segment so an in-module unit test under a module path still
/// counts.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|leaf| leaf.starts_with(ACCEPTANCE_PREFIX))
}

fn finished(line: &str) -> Option<&'static str> {
    if line.starts_with("ok") {
        Some("pass")
    } else if line.starts_with("ignored") || line.starts_with("ignored,") {
        Some("unknown")
    } else if line.starts_with("FAILED") || line.starts_with("failed") {
        Some("fail")
    } else {
        None
    }
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, found)| *found == status)
        .count() as u64
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact, as `(relative path, sha256, kind)`: it must sit
/// under the evidence directory, and it is hashed in place with the production
/// SHA-256 (the validator re-hashes it with `hashlib` independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .strip_prefix(evidence_dir)
        .unwrap_or_else(|_| {
            panic!(
                "artifact {} must sit under the evidence directory {}",
                source.display(),
                evidence_dir.display()
            )
        })
        .to_string_lossy()
        .replace('\\', "/");
    let bytes =
        fs::read(source).unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
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
                 \"windowed-run.json\"]}}",
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
