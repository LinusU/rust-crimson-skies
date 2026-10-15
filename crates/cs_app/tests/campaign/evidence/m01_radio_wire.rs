//! Evidence-report harness for task `M01-RADIO-SAMPLE-WIRE` (#1466):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! A member of the campaign binary (see `evidence.rs`), deliberately not named
//! `accept_m01_radio_wire_*`: it is not part of the acceptance suite and fails
//! loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_radio_wire_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-RADIO-SAMPLE-WIRE/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-RADIO-SAMPLE-WIRE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_radio_wire_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked --test campaign evidence_report_m01_radio_wire -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-RADIO-SAMPLE-WIRE/acceptance.json
//!    --artifact-root private/evidence/M01-RADIO-SAMPLE-WIRE --require-pass`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-RADIO-SAMPLE-WIRE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness runs M01's real stage and
//! headless composition (`stage_for` → `build_headless`), pumps it past
//! `OBJECTIVE1`'s measured two-second wake, and reads the **device's own
//! library** and lookup for the opening conversation's members — ids, digests,
//! counts and refusal codes as JSON, no original bytes. That is a real
//! production run over the owner's installation through the very code this
//! task wired (`install_radio_voice_publication` → `publish_prepared_voices` →
//! `publish_voice_members`), not a paraphrase of the acceptance assertions.
//!
//! The test counts and the assertions come from the **prefixed test lines**
//! only, never from the `test result:` summaries: a summary aggregates every
//! test binary cargo ran, so reading it would report hundreds of unrelated
//! tests as this task's acceptance selection.

use super::*;

use std::sync::Arc;

use bevy::app::App;
use cs_app::audio::{
    AudibleDevice, CODE_DEVICE_CLOSED, CODE_SAMPLE_UNAVAILABLE, DeviceSampleLibrary, PcmAudio,
    SampleLibrary,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHostReport, RadioLineOutcome, build_headless, dialogue_content_id, stage_for, teardown,
};
use cs_sim::audio_events::{AudioBus, AudioDevice, RADIO_GAIN, VoiceStart};

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_radio_wire_";

/// The container M01's voice members are read from (the labelled choice #1438
/// measured; which of `soundsh`/`soundsl` the original opens is unmeasured,
/// F14-D.7).
const CONTAINER: &str = "ZBD/soundsh.zbd";

/// The two voice members of M01's opening crew conversation, measured by #1438.
const OPENING_MEMBERS: [&str; 2] = [
    "VO_c2-NW-m1_WorkersVoyage_1.wav",
    "VO_c2-NW-m1_Zachary_2.wav",
];

/// The group `OBJECTIVE1`'s measured wake cues.
const OPENING_GROUP: &str = "snd_NW1Start";

/// A member name this container does not declare, used to show the device's own
/// lookup telling delivered from never-delivered.
const ABSENT_MEMBER: &str = "VO_c2-NW-m1_nobody_99.wav";

/// One start on the radio bus at unity gain, as the mixer emits it.
fn radio_start(asset: ContentId) -> VoiceStart {
    VoiceStart {
        asset,
        bus: AudioBus::Radio,
        gain: RADIO_GAIN,
        pan: 0.0,
        pitch: 1.0,
    }
}

/// What the second production observation measured.
#[derive(Debug)]
struct Census {
    /// How many lines the prepare step prepared from the retail table.
    prepared_lines: usize,
    /// How many decoded voice members it kept for them.
    retained_voices: usize,
    /// The tick the composed run reached.
    tick: u64,
    /// `(member, dialogue id, frames, rate_hz, channels, device refusal code)`
    /// for each opening member, in the order the table spells them.
    opening: Vec<(String, String, u64, u32, u16, &'static str)>,
    /// The refusal code the device gives for a member nothing published.
    undelivered_refusal: &'static str,
}

impl Census {
    /// The census as JSON: ids, digests and counts, no original bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        let opening: Vec<String> = self
            .opening
            .iter()
            .map(|(member, id, frames, rate, channels, refusal)| {
                format!(
                    "  {{\"member\": {}, \"dialogue_id\": {}, \"frames\": {frames}, \
                     \"rate_hz\": {rate}, \"channels\": {channels}, \
                     \"device_refusal\": {}, \"is_sample_unavailable\": {}}}\n",
                    jstr(member),
                    jstr(id),
                    jstr(refusal),
                    *refusal == CODE_SAMPLE_UNAVAILABLE,
                )
            })
            .collect();
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"container\": {},\n \
             \"group\": {},\n \"prepared_lines\": {},\n \"retained_voices\": {},\n \
             \"composition_tick\": {},\n \"opening\": [\n{}],\n \
             \"undelivered_refusal\": {},\n \"undelivered_is_sample_unavailable\": {},\n \
             \"claim\": \"implemented\",\n \"capabilities\": [\"retail\", \"synthetic\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(CONTAINER),
            jstr(OPENING_GROUP),
            self.prepared_lines,
            self.retained_voices,
            self.tick,
            opening.join(","),
            jstr(self.undelivered_refusal),
            self.undelivered_refusal == CODE_SAMPLE_UNAVAILABLE,
        )
    }
}

/// The library the composition's device plays from.
fn composition_library(app: &App) -> Arc<dyn SampleLibrary> {
    Arc::clone(app.world().resource::<DeviceSampleLibrary>().library())
}

/// Pumps the composed run one committed fixed tick per world update until the
/// opening group's cue has been offered, and answers the last tick with every
/// cue outcome seen.
fn pump_to_opening(app: &mut App) -> (u64, Vec<RadioLineOutcome>) {
    let timestep = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep();
    let mut outcomes: Vec<RadioLineOutcome> = Vec::new();
    let mut tick = 0u64;
    let mut seen = 0u64;
    for _ in 0..200_000 {
        *app.world_mut()
            .resource_mut::<bevy::time::TimeUpdateStrategy>() =
            bevy::time::TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let Some(report) = app.world().get_resource::<MissionHostReport>() else {
            continue;
        };
        tick = report.tick.0;
        if tick == seen {
            continue;
        }
        seen = tick;
        match &report.answer {
            Ok(produced) => outcomes.extend(produced.radio.iter().cloned()),
            Err(error) => panic!("the composed step at tick {tick} refused: {error}"),
        }
        if tick >= 600
            || (tick >= 245
                && outcomes.iter().any(|outcome| {
                    matches!(outcome, RadioLineOutcome::Queued { group, .. } if group == OPENING_GROUP)
                }))
        {
            break;
        }
    }
    (tick, outcomes)
}

/// The second production observation: compose M01, pump it past the opening
/// wake, and read the opening conversation's members out of the device library
/// the publish step filled.
fn observe(game_dir: &Path) -> Census {
    let label = MissionLabel::new("M01").expect("M01 is a valid mission label");
    let plan = plan_mission_launch(game_dir, label, "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    drop(plan);

    let prepared_lines = stage.host.radio.prepared().count();
    let retained_voices = stage.host.radio.voices().len();
    assert!(prepared_lines > 0, "M01's prepare step prepared no line");
    assert_eq!(
        prepared_lines, retained_voices,
        "the prepare step must keep the PCM it decoded for every line it prepared"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let (tick, outcomes) = pump_to_opening(&mut app);
    assert!(tick >= 245, "the composed run reached only tick {tick}");

    let queued: Vec<&str> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            RadioLineOutcome::Queued { group, member, .. } if group == OPENING_GROUP => {
                Some(member.as_str())
            }
            _ => None,
        })
        .collect();

    let library = composition_library(&app);
    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let mut opening = Vec::new();
    for member in OPENING_MEMBERS {
        assert!(
            queued.contains(&member),
            "the opening group must queue {member:?}; it queued {queued:?}"
        );
        let id = dialogue_content_id(CONTAINER, member).expect("the archive spells the member");
        let pcm: PcmAudio = library
            .pcm(&id)
            .unwrap_or_else(|| panic!("{member:?} did not reach the device library under {id}"));
        assert!(pcm.frames() > 0, "member {member:?} decoded to no frames");
        let refusal = device
            .start_one_shot(radio_start(id.clone()))
            .expect_err("an unopened device grants no output")
            .code;
        assert_eq!(
            refusal, CODE_DEVICE_CLOSED,
            "the device must resolve {member:?} and refuse only the output"
        );
        opening.push((
            member.to_owned(),
            id.as_str().to_owned(),
            pcm.frames(),
            pcm.rate_hz(),
            pcm.channels(),
            refusal,
        ));
    }
    let absent =
        dialogue_content_id(CONTAINER, ABSENT_MEMBER).expect("the absent member derives an id");
    let undelivered_refusal = device
        .start_one_shot(radio_start(absent))
        .expect_err("a member nothing published is refused")
        .code;
    assert_eq!(undelivered_refusal, CODE_SAMPLE_UNAVAILABLE);
    teardown(&mut app);

    Census {
        prepared_lines,
        retained_voices,
        tick,
        opening,
        undelivered_refusal,
    }
}

/// How this run was reviewed; the measured counts are interpolated from the
/// census this same run produced, so a report cannot describe another run's
/// numbers.
fn review_method(census: &Census, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         (m01-radio-wire-census.json) that composes M01 through `stage_for`/`build_headless`, pumps \
         it past OBJECTIVE1's wake and reads the device's own library and lookup. Claim is \
         implemented only, not checked: no independent reviewer has looked at this run, no original \
         executable ran and no person heard anything. MEASURED (retail, read-only): the prepare step \
         prepared {prepared} line(s) and kept {voices} decoded voice member(s); the composed run \
         reached tick {tick}; the opening group's {opening} member(s) reached the device library \
         under the dialogue id their line names and the device's own lookup resolves them (only the \
         absent output refuses, {delivered}), while an id nothing published is refused \
         {undelivered} by name. LIMITS OF WHAT WAS MEASURED: the identity rule and route (b) are the \
         owner's designed decision (task #1465 note of 2026-10-15) because the installation states \
         no cue class (F14-D.7), so nothing here is verified_original; which of ZBD/soundsh.zbd and \
         ZBD/soundsl.zbd the original opens is unmeasured, so the container is the labelled choice \
         #1438 made; this is decoded-PCM evidence, not audible evidence — the F41-D listening test \
         stays the owner's gate; and the composed run's world/presentation path needs \
         CS_ENGINE_IMAGE (M01's player start), so this observation needs it too. The {tests} \
         assertion(s) discovered under this task's own prefix are the tests of this report. \
         Validated with tools/validate_evidence.py --require-pass.",
        prepared = census.prepared_lines,
        voices = census.retained_voices,
        tick = census.tick,
        opening = census.opening.len(),
        delivered = census
            .opening
            .first()
            .map_or("<none>", |(_, _, _, _, _, code)| code),
        undelivered = census.undelivered_refusal,
        tests = tests,
    )
}

/// One assertion's entry: both artifacts of this report carry it.
fn wire_assertions(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"m01-radio-wire-census.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

/// The acceptance run's own counts, keyed by this task's prefix only.
#[derive(Debug, Default)]
struct WireSuite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output, ignoring every `test result:` summary (see the module note).
fn parse_wire_suite(log: &str) -> WireSuite {
    let mut suite = WireSuite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = wire_status(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            wire_record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            let tail = &after[separator + 5..];
            cursor = tail;
            if !full.contains(ACCEPTANCE_PREFIX) {
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            match wire_status(tail.trim()) {
                Some(status) => wire_record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = wire_count(&suite, "pass");
    suite.failed = wire_count(&suite, "fail");
    suite.ignored = wire_count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// The libtest result word at the head of a test's tail line.
fn wire_status(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn wire_count(suite: &WireSuite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
}

fn wire_record(suite: &mut WireSuite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn evidence_report_m01_radio_wire_writes_the_acceptance_report() {
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
    assert!(
        std::env::var("CS_ENGINE_IMAGE").is_ok_and(|path| !path.is_empty()),
        "CS_ENGINE_IMAGE must name the owner's decrypted image: M01's player start is only known \
         from it, so the composed observation cannot run without it"
    );

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_wire_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("m01-radio-wire-census.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write m01-radio-wire-census.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-RADIO-SAMPLE-WIRE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {}}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        census.tick,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        wire_assertions(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(&review_method(&census, suite.discovered as usize)),
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
