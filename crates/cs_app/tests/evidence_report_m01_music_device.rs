//! Evidence-report harness for task `M01-MUSIC-DEVICE` (#1547):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_music_device_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_app -- accept_m01_music_device_ --include-ignored
//!    2>&1 | tee private/evidence/M01-MUSIC-DEVICE/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-MUSIC-DEVICE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app -- accept_m01_music_device_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_CAPABILITIES=retail,gpu,audio CS_GAME_DIR=<installation> \
//!      cargo test --locked --release -p cs_app --test evidence_report_m01_music_device -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/M01-MUSIC-DEVICE/acceptance.json
//!    --artifact-root private/evidence/M01-MUSIC-DEVICE --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-MUSIC-DEVICE.json`.
//!
//! The second artifact, `music-playback.json`, is a **real device probe**: the
//! harness decodes M01's music tracks through `MissionRadio::read`, opens the
//! machine's real output with `open_audible_device` (refused by name without
//! the `audio` capability), drives M01's own cue chain, mixes the director's
//! verdicts to that device and reads what the output stream pulled from the
//! sounding track. It is the **source boundary** — frames the stream consumed —
//! never a capture of a speaker; hearing it stays the owner's `human_review`.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cs_app::audio::{
    AudioSession, CapabilityDeclaration, InMemorySamples, MusicSelector, SampleProbe,
    open_audible_device, publish_music_members,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionRadio, ObjectiveLifecycles, RadioLineOutcome, declared_engine_objectives,
    declared_sound_groups, offer_cues,
};
use cs_app::objectives::recover_retail_objectives;
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::SceneGeneration;
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::audio_events::{AudioAssetSpec, AudioMixer, AudioRouter};
use cs_types::net::SessionId;

/// Every acceptance test the report must see pass: this task's own prefix.
const ACCEPTANCE_PREFIX: &str = "accept_m01_music_device_";

/// The fixed seed the probe's track selection uses (designed stream).
const SELECTOR_SEED: u64 = 0;

/// What the device probe measured.
#[derive(Debug)]
struct Playback {
    /// Tracks the prepare step decoded and published.
    published: usize,
    /// Requests M01's chain served, as `(group, member, asset)`.
    requested: Vec<(String, String, String)>,
    /// The track the device ended up sounding.
    sounding: String,
    /// The sounding track's decoded frames and rate.
    frames: u64,
    rate_hz: u32,
    channels: u16,
    /// Sample values the output stream pulled and the peak among them.
    pulled: u64,
    peak: f32,
}

impl Playback {
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        let requested: Vec<String> = self
            .requested
            .iter()
            .map(|(group, member, asset)| {
                format!(
                    "  {{\"group\": {}, \"member\": {}, \"asset\": {}}}",
                    jstr(group),
                    jstr(member),
                    jstr(asset)
                )
            })
            .collect();
        format!(
            "{{\n \"boundary\": \"sample_probe_source_pull\",\n \"install_sha256\": {},\n \
             \"candidate_tree\": {},\n \"selector_seed\": {SELECTOR_SEED},\n \
             \"tracks_published\": {},\n \"requested\": [\n{}\n ],\n \"sounding\": {},\n \
             \"channels\": {},\n \"rate_hz\": {},\n \"member_frames\": {},\n \
             \"sample_values_pulled\": {},\n \"peak\": {},\n \"bus\": \"music\",\n \
             \"looping\": \"designed: loop/fade rules are unmeasured\",\n \
             \"backend\": \"audible\",\n \"verified_original\": false,\n \
             \"human_review\": false\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            self.published,
            requested.join(",\n"),
            jstr(&self.sounding),
            self.channels,
            self.rate_hz,
            self.frames,
            self.pulled,
            if self.peak.is_finite() {
                self.peak.to_string()
            } else {
                "null".to_owned()
            },
        )
    }
}

/// The device probe: M01's music cue chain, mixed to the real output.
fn observe(game_dir: &Path) -> Playback {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M06").expect("M01 is a valid mission label"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(game_dir, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let radio = MissionRadio::read(game_dir, declared_sound_groups(&engine))
        .expect("M01's sound groups read from sounds.zrd");

    let library = Arc::new(InMemorySamples::new());
    let decoded: Vec<_> = radio
        .music_tracks()
        .iter()
        .map(|track| {
            radio
                .decode_music(&track.id)
                .unwrap_or_else(|error| panic!("{} decodes: {error}", track.member))
        })
        .collect();
    let published = publish_music_members(library.as_ref(), &decoded);
    assert!(published > 0, "M01's music tracks decode and publish");
    let probe = Arc::new(SampleProbe::new());
    let mut device =
        open_audible_device(library.clone(), &CapabilityDeclaration::from_environment())
            .unwrap_or_else(|error| panic!("the audible device was refused: {error}"))
            .with_probe(probe.clone());

    let mut music_blocks = BTreeSet::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if objective
            .declaration
            .wakeup_sound_group
            .as_deref()
            .is_some_and(|group| cs_app::audio::music_state_spec(group).is_some())
        {
            music_blocks.insert(index);
        }
    }
    let mut completers = BTreeSet::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if objective
            .declaration
            .wake
            .targets()
            .iter()
            .any(|target| music_blocks.contains(target))
        {
            completers.insert(index);
        }
    }

    let mut lifecycles = ObjectiveLifecycles::engine(engine);
    let mut selector = MusicSelector::launch(SELECTOR_SEED, radio.music_arms());
    let session = SessionId::new(9).expect("a nonzero session");
    let mut audio = AudioSession::new(
        AudioRouter::new(session),
        SceneGeneration(1),
        std::iter::empty::<AudioAssetSpec>(),
    );
    let mut mixer = AudioMixer::new(session);
    let mut requested = Vec::new();
    let mut serve = |lifecycles: &mut ObjectiveLifecycles| {
        for outcome in offer_cues(
            &lifecycles.take_cues(),
            &radio,
            &mut selector,
            Some(&mut audio),
        ) {
            if let RadioLineOutcome::MusicRequested {
                group,
                member,
                asset,
                ..
            } = outcome
            {
                requested.push((group, member, asset.as_str().to_owned()));
            }
        }
        let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
        assert!(report.refusals.is_empty(), "{:?}", report.refusals);
    };
    for _ in 0..4 * BASELINE_FIXED_HZ {
        lifecycles.advance(1.0 / f64::from(BASELINE_FIXED_HZ));
        serve(&mut lifecycles);
    }
    for index in completers {
        lifecycles.complete(index as u32);
        serve(&mut lifecycles);
    }
    assert!(!requested.is_empty(), "M01 requests music states");

    // Let the output stream pull from the sounding track.
    std::thread::sleep(Duration::from_millis(2000));
    let sounding = mixer
        .music_playing()
        .cloned()
        .expect("the device is sounding the last requested track");
    let pcm = cs_app::audio::SampleLibrary::pcm(library.as_ref(), &sounding)
        .expect("the sounding track is in the library");
    let pulled = probe.pulled();
    let peak = probe.peak();
    let _ = mixer.device_lost(&mut device);
    assert!(
        pulled > 0 && peak > 0.0,
        "the output stream pulled nothing audible from {sounding} ({pulled} samples, peak {peak})"
    );
    let last = requested.last().expect("a request").clone();
    assert_eq!(
        last.2,
        sounding.as_str(),
        "the device sounds the last request"
    );
    Playback {
        published,
        requested,
        sounding: sounding.as_str().to_owned(),
        frames: pcm.frames(),
        rate_hz: pcm.rate_hz(),
        channels: pcm.channels(),
        pulled,
        peak,
    }
}

/// How this run was reviewed.
fn review_method(playback: &Playback, tests: u64) -> String {
    format!(
        "Acceptance suite and a real-device probe run locally with the retail and audio \
         capabilities; every field is derived from the recorded log, production discovery of \
         $CS_GAME_DIR, and music-playback.json, which decodes M01's music tracks ({} published), \
         re-executes M01's music cue chain ({} request(s)), mixes the director's verdicts to the \
         machine's real output and records what the output stream pulled from the sounding track \
         ({} sample values, peak {}). Claim is implemented only, not checked: no independent \
         reviewer has looked at this run, no original executable ran and no person heard \
         anything (human_review). The probe is the source boundary, not a speaker capture. \
         DESIGNED, not measured: the track loops, a transition is a hard cut, gain is unity on \
         the music bus, and the slot-to-track binding is the image table's spelled order \
         (docs/findings/2026-10-16-m01-play-music-state-requests.md unknowns 1, 5, 8). The \
         music cues are not in M01's opening, so the probe completes the waker blocks exactly as \
         the #1463 harness does. The {tests} assertion(s) under this task's own prefix are the \
         tests of this report. Validated with tools/validate_evidence.py --require-pass.",
        playback.published,
        playback.requested.len(),
        playback.pulled,
        playback.peak,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_music_device_writes_the_acceptance_report() {
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
    fs::create_dir_all(&evidence_dir)
        .unwrap_or_else(|error| panic!("create {}: {error}", evidence_dir.display()));

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

    // The real-device probe: M01's music cue chain mixed to the output.
    let playback = observe(&game_dir);
    let playback_path = evidence_dir.join("music-playback.json");
    fs::write(
        &playback_path,
        playback.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write music-playback.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&playback_path, "audio", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-MUSIC-DEVICE\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": {SELECTOR_SEED},\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"audio\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&reviewer),
        jstr(&review_method(&playback, suite.discovered)),
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_play_music.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-PLAY-MUSIC` written relative
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

/// Extracts the per-test results of the `accept_m01_music_` tests from a
/// recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test
/// result:` summaries: a summary aggregates every test binary cargo ran, so
/// reading it would report hundreds of unrelated tests as this task's
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

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
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
                 \"music-census.json\"]}}",
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
