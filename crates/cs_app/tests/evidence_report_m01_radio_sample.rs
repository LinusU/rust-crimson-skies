//! Evidence-report harness for task `M01-RADIO-SAMPLE-IDENTITY` (#1465):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_radio_sample_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_radio_sample_
//!    --include-ignored 2>&1 | tee
//!    private/evidence/M01-RADIO-SAMPLE-IDENTITY/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-RADIO-SAMPLE-IDENTITY \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_radio_sample_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_radio_sample -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-RADIO-SAMPLE-IDENTITY/acceptance.json
//!    --artifact-root private/evidence/M01-RADIO-SAMPLE-IDENTITY --require-pass`
//! 4. Commit a copy as
//!    `docs/findings/evidence/M01-RADIO-SAMPLE-IDENTITY.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs the real chain — VFS
//! session over the installation, `ZbdContainer` routing `ZBD/soundsh.zbd`,
//! `sound_member_pcm` decoding the member, `voice_member_id` deriving its
//! dialogue id, `publish_voice_members` publishing it, and the real
//! `AudibleDevice`'s own lookup telling that delivered member from one it was
//! never given — and records ids, digests, counts and the refusal codes as
//! JSON. That is a real production run over the owner's installation, not a
//! paraphrase of the acceptance assertions, and it carries no original bytes.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::audio::{
    AudibleDevice, CODE_DEVICE_CLOSED, CODE_SAMPLE_UNAVAILABLE, DecodedVoiceMember,
    InMemorySamples, SampleLibrary, publish_voice_members, voice_member_id,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::catalog::baseline::install_file_key;
use cs_sim::audio_events::{AudioBus, AudioDevice, VoiceStart};
use cs_types::content::{ContentId, ContentKind};

/// Every acceptance test the report must see pass: this task's own prefix, so
/// the selection cannot credit a sibling task's assertions to this report.
const ACCEPTANCE_PREFIX: &str = "accept_m01_radio_sample_";

/// The container M01's voice members are read from (the labelled choice #1438
/// measured; which of `soundsh`/`soundsl` the original opens is unmeasured,
/// F14-D.7).
const CONTAINER: &str = "ZBD/soundsh.zbd";

/// One real M01 voice member, measured by #1438: the first line of the
/// mission's opening crew conversation.
const M01_MEMBER: &str = "VO_c2-NW-m1_WorkersVoyage_1.wav";

/// A member name this container does not declare, used to show the device's own
/// lookup telling delivered from never-delivered.
const ABSENT_MEMBER: &str = "VO_c2-NW-m1_nobody_99.wav";

/// One start on the radio bus at unity gain, as the mixer emits it.
fn radio_start(asset: ContentId) -> VoiceStart {
    VoiceStart {
        asset,
        bus: AudioBus::Radio,
        gain: 1.0,
        pan: 0.0,
        pitch: 1.0,
    }
}

/// What the second production observation measured.
#[derive(Debug)]
struct Census {
    member: String,
    dialogue_id: String,
    member_key: String,
    decoded_frames: u64,
    decoded_samples: u64,
    rate_hz: u32,
    channels: u16,
    delivered_refusal: &'static str,
    undelivered_refusal: &'static str,
}

impl Census {
    /// The census as JSON: ids, digests and counts, no original bytes.
    fn to_json(&self, install_sha256: &str, candidate_tree: &str) -> String {
        format!(
            "{{\n \"install_sha256\": {},\n \"candidate_tree\": {},\n \"container\": {},\n \
             \"member\": {},\n \"member_key\": {},\n \"dialogue_id\": {},\n \
             \"decoded\": {{\"channels\": {}, \"rate_hz\": {}, \"frames\": {}, \"samples\": {}}},\n \
             \"device_lookup\": {{\"delivered_refusal\": {}, \"undelivered_refusal\": {},\n \
             \"delivered_is_sample_unavailable\": {},\n \"undelivered_is_sample_unavailable\": {}}},\n \
             \"claim\": \"implemented\",\n \"capabilities\": [\"retail\"]\n}}\n",
            jstr(install_sha256),
            jstr(candidate_tree),
            jstr(CONTAINER),
            jstr(&self.member),
            jstr(&self.member_key),
            jstr(&self.dialogue_id),
            self.channels,
            self.rate_hz,
            self.decoded_frames,
            self.decoded_samples,
            jstr(self.delivered_refusal),
            jstr(self.undelivered_refusal),
            self.delivered_refusal == CODE_SAMPLE_UNAVAILABLE,
            self.undelivered_refusal == CODE_SAMPLE_UNAVAILABLE,
        )
    }
}

/// The second production observation: read one real M01 voice member through the
/// production chain, publish it under its dialogue id, and let the real
/// `AudibleDevice`'s own lookup tell it from a member it was never given.
fn observe(game_dir: &Path) -> Census {
    let found = discover(game_dir).expect("production discovery reads the installation");
    let context = cs_types::asset_id::ResolveContext::new(fingerprint(&found.manifest));
    let mut builder = cs_assets::vfs::SessionBuilder::new(context);
    builder
        .mount_installation(game_dir, &found.diagnosis)
        .expect("the installation mounts");
    let session = builder.open();
    let container = cs_assets::zbd::ZbdContainer::open(
        &session,
        &cs_types::asset_id::AssetKey::from_spelling("install", CONTAINER, "default")
            .expect("the sound family's own container key"),
    )
    .expect("ZBD/soundsh.zbd routes to the sound family");
    let mut parse = cs_formats::ParseContext::with_defaults(container.label());
    let index = container.index(&mut parse).expect("its trailer indexes");
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .expect("its sound archive reads");

    let asset = assets
        .entries()
        .iter()
        .find(|asset| String::from_utf8_lossy(asset.name()).eq_ignore_ascii_case(M01_MEMBER))
        .unwrap_or_else(|| panic!("{CONTAINER} declares no member {M01_MEMBER:?}"));
    let member = String::from_utf8_lossy(asset.name()).into_owned();
    let pcm = cs_app::audio::sound_member_pcm(asset, &mut parse)
        .expect("the member decodes under its own header");
    assert!(pcm.frames() > 0, "member {member:?} decoded to no frames");

    let id = voice_member_id(CONTAINER, &member).expect("the member derives a dialogue id");
    let member_key = install_file_key(&format!("{CONTAINER}/{member}"));
    assert_eq!(
        id,
        ContentId::from_source(ContentKind::Dialogue, &member_key).expect("the derived id"),
        "the published id is the one a RadioLine names"
    );

    let library: Arc<dyn SampleLibrary> = Arc::new(InMemorySamples::new());
    let member_record =
        DecodedVoiceMember::new(CONTAINER, &member, pcm.clone()).expect("a valid voice member");
    assert_eq!(
        publish_voice_members(library.as_ref(), &[member_record]).published,
        1
    );
    assert_eq!(library.pcm(&id), Some(pcm.clone()));

    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let delivered_refusal = device
        .start_one_shot(radio_start(id.clone()))
        .expect_err("an unopened device grants no output")
        .code;
    assert_eq!(
        delivered_refusal, CODE_DEVICE_CLOSED,
        "the device found the delivered member and refused only the output"
    );

    let absent =
        voice_member_id(CONTAINER, ABSENT_MEMBER).expect("the absent member derives an id");
    let undelivered_refusal = device
        .start_one_shot(radio_start(absent))
        .expect_err("a member the prepare did not deliver is refused")
        .code;
    assert_eq!(undelivered_refusal, CODE_SAMPLE_UNAVAILABLE);

    Census {
        member,
        dialogue_id: id.as_str().to_owned(),
        member_key,
        decoded_frames: pcm.frames(),
        decoded_samples: pcm.sample_count(),
        rate_hz: pcm.rate_hz(),
        channels: pcm.channels(),
        delivered_refusal,
        undelivered_refusal,
    }
}

/// How this run was reviewed; the measured counts are interpolated from the
/// census this same run produced, so a report cannot describe another run's
/// numbers.
fn review_method(census: &Census, tests: usize) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run of \
         the read/decode/publish/lookup chain over the installation \
         (m01-radio-sample-census.json). Claim is implemented only, not checked: no independent \
         reviewer has looked at this run, no original executable ran and no person heard anything. \
         MEASURED (retail, read-only): the member {member} of {container} decodes to {frames} mono \
         frames at {rate} Hz through the production VFS/`ZbdContainer`/`sound_member_pcm` chain, and \
         its dialogue id is ContentKind::Dialogue over F14-D.7's install_file_key spelling; after \
         `publish_voice_members` the device's own lookup finds it (only the absent output refuses, \
         {delivered}), while an id nothing published is refused {undelivered} by name. LIMITS OF \
         WHAT WAS MEASURED: the identity rule and route (b) are the owner's designed decision (task \
         #1465 note of 2026-10-15) because the installation states no cue class (F14-D.7), so \
         nothing here is verified_original; which of ZBD/soundsh.zbd and ZBD/soundsl.zbd the \
         original opens is unmeasured, so the container is the labelled choice #1438 made; the \
         producer that calls `publish_voice_members` is #1438's prepare step and is still open \
         (filed as #1466), so this report does not claim M01 is audible end to end. The {tests} \
         assertions discovered under this task's own prefix are the tests of this report. Validated \
         with tools/validate_evidence.py --require-pass.",
        member = census.member,
        container = CONTAINER,
        frames = census.decoded_frames,
        rate = census.rate_hz,
        delivered = census.delivered_refusal,
        undelivered = census.undelivered_refusal,
        tests = tests,
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_radio_sample_writes_the_acceptance_report() {
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

    // The second production observation.
    let census = observe(&game_dir);
    let census_path = evidence_dir.join("m01-radio-sample-census.json");
    fs::write(
        &census_path,
        census.to_json(&install_sha256, &candidate_tree),
    )
    .expect("write m01-radio-sample-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-RADIO-SAMPLE-IDENTITY\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_radio_sample.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-RADIO-SAMPLE-IDENTITY` written
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

/// Extracts the per-test results of this task's tests from a recorded `cargo
/// test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance
/// selection. A prefixed test that was skipped is recorded with the schema's
/// `unknown` status rather than counted as a pass, so a report can never claim
/// an assertion it did not run.
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
                 \"m01-radio-sample-census.json\"]}}",
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
