//! Evidence-report harness for task #635 (`M01-LC-AUDIO-DEVICE`):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_lc_audio_device_*`: it is not part of the acceptance
//! suite and fails loudly when its inputs are missing.
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m01_lc_audio_device_ \
//!      --include-ignored 2>&1 | tee private/evidence/M01-LC-AUDIO-DEVICE/cargo-test.log
//!    ```
//!    (note the exit status; the retail scenario inside takes ~15 minutes)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-LC-AUDIO-DEVICE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_audio_device_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_lc_audio_device -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M01-LC-AUDIO-DEVICE/acceptance.json \
//!      --artifact-root private/evidence/M01-LC-AUDIO-DEVICE
//!    ```
//!    **Without `--require-pass`**, deliberately: that flag rejects any nonempty
//!    `unknowns`, and this report's `unknowns` holds the scope limits that gate
//!    every audible and fidelity claim. Removing them to satisfy the flag is the
//!    shortcut `docs/contracts/CLI-EVIDENCE.md` forbids. The same choice, for the
//!    same reason, is documented in
//!    `docs/findings/2026-09-29-f12-j-letter-o-colour.md`.
//! 4. Commit a copy as `docs/findings/evidence/M01-LC-AUDIO-DEVICE.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. The second artifact is
//! a **second production observation** over the installation: this harness reads
//! `ZBD/soundsl.zbd`'s own sound archive through the production ZBD reader and
//! records its member census (count, and the readiness distribution). That is a
//! real production run over the original data, not a paraphrase of the
//! acceptance assertions.
//!
//! The claim is `implemented`, never `verified_original`: the harness measures
//! that a decoded member reached a running output stream on a machine declaring
//! `audio`, which is not evidence that a person heard it. See
//! `docs/findings/2026-10-05-m01-lc-audible-audio-device.md`.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_lc_audio_device_";

/// The task key, which is also the evidence directory's name.
const TASK: &str = "M01-LC-AUDIO-DEVICE";

/// The sound container the census reads: the smaller of the retail installation's
/// two sound archives, so the census is bounded.
const SOUND_CONTAINER: &str = "ZBD/soundsl.zbd";

const REVIEW_METHOD: &str = "Acceptance suite run locally on a machine declaring retail and audio. \
     This harness derives every field from the recorded log, production discovery of $CS_GAME_DIR, \
     rustc and Cargo.lock, plus a second production run: it reads the ZBD sound archive of \
     ZBD/soundsl.zbd through cs_assets::zbd::ZbdContainer and records its member census \
     (sound-member-census.json). LIMITS OF WHAT WAS PROVEN, each recorded in \
     docs/findings/2026-10-05-m01-lc-audible-audio-device.md and each a real limit of the \
     measurement rather than a caveat about the tooling: (1) no person heard anything — the \
     acceptance scenario taps the source boundary of a running output stream, which is stronger \
     than a decoded WAV and weaker than the human_review gate this workspace cannot supply; \
     (2) only one member of one container was decoded and played, so the complete media audit \
     F41-D asks for is not done and no claim is made that the other members of this archive, or \
     any member of ZBD/soundsh.zbd, plays; (3) bus faders are not applied, because no original bus \
     fader is known, and there is no limiter, so two voices summing past 1.0 clip at the device; \
     (4) doppler is absent rather than designed: VoiceUpdate carries gain, pan and pitch and \
     nothing carries a velocity-dependent frequency shift; (5) the constant-power pan law and the \
     mono-to-stereo upmix are designed, not measured, and the original engine's spatialization is \
     unmeasured; (6) no retail member declares loop points, so every loop repeats an asset \
     end-to-end seam and all, and whether the original did the same is unknown; (7) the $CS_\
     CAPABILITIES declaration is trusted as exactly that — a declaration is not proof a speaker \
     works — and the audio capability was exercised, not human_play or human_review; \
     (8) the last hop from a delivered F15 load closure to a populated SampleLibrary is not wired \
     yet, so the plugin is handed a library by a caller while the mixer, plugin and device are all \
     real and tested. The census read every member the archive declares and left none unresolved, so \
     `unknowns` names only these scope limits, not a gap in the reading. THIS REPORT IS VALIDATED \
     WITH tools/validate_evidence.py WITHOUT --require-pass: that flag rejects any nonempty \
     `unknowns`, and this task's acceptance criterion is that these limits stay recorded — deleting \
     them to satisfy the flag would be exactly the shortcut docs/contracts/CLI-EVIDENCE.md forbids. \
     The same choice, for the same reason, is documented in \
     docs/findings/2026-09-29-f12-j-letter-o-colour.md.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_lc_audio_device_writes_the_acceptance_report() {
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
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the sound archive's own member census.
    let census = sound_census(&game_dir);
    assert!(
        census.total > 0,
        "the census read no member at all: {census:?}"
    );
    let readiness: Vec<String> = census
        .readiness
        .iter()
        .map(|(code, count)| format!("{{\"code\": {}, \"members\": {count}}}", jstr(code)))
        .collect();
    let census_path = evidence_dir.join("sound-member-census.json");
    fs::write(
        &census_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"container\": {}, \"members\": {}, \
              \"decoded\": {}, \"readiness\": [{}], \"first_decoded_member\": {}, \
              \"first_decoded_frames\": {}, \"first_decoded_rate_hz\": {}, \
              \"first_decoded_channels\": {}}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            jstr(SOUND_CONTAINER),
            census.total,
            census.decoded,
            readiness.join(", "),
            option_str(census.first_decoded.as_deref()),
            census.first_frames,
            census.first_rate_hz,
            census.first_channels,
        ),
    )
    .expect("write sound-member-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": {},\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"audio\", \"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(TASK),
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array_owned(&argv),
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
        str_array(&UNKNOWN_LIMITS),
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
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

/// The scope limits the review method names, as machine-readable unknowns.
///
/// Each is a limit of what was measured, not a defect in the measurement: the
/// census and the acceptance run left nothing unresolved *about what they
/// measured*. None of these may be removed to make a validator happier; they are
/// what gates a fidelity or audible claim.
const UNKNOWN_LIMITS: [&str; 8] = [
    "whether a person heard the decoded member: the tap is the source boundary of a running \
     output stream, and human_review is the owner's gate (F41-D)",
    "the complete media audit: only one member of ZBD/soundsl.zbd was decoded and played, and \
     ZBD/soundsh.zbd was never opened",
    "the original bus faders: none is applied, because none is known",
    "the original's spatialization law and whether it pans as a constant-power stereo gain",
    "the original's loop seams: no retail member declares loop points, so every loop here repeats \
     an asset end-to-end",
    "the original's doppler: absent from this backend, and therefore neither evidence-backed nor \
     a designed option (F41 non-negotiable behavior 1)",
    "the original's response to two voices summing past full scale: there is no limiter here",
    "the hop from a delivered F15 load closure to a populated SampleLibrary: the device, plugin \
     and mixer are wired, but no loader populates one yet",
];

/// What one production read of a sound archive produced.
#[derive(Debug)]
struct Census {
    /// How many members the archive's own index declared and that read.
    total: usize,
    /// How many decoded under their own declared format.
    decoded: usize,
    /// The readiness code each member reported, with how many carried it.
    readiness: BTreeMap<String, usize>,
    /// The first member that decoded, named as the container's own index spells it.
    first_decoded: Option<String>,
    first_frames: u64,
    first_rate_hz: u32,
    first_channels: u16,
}

/// Reads one sound container through the production ZBD reader and censuses it.
///
/// This is the second production observation the report carries: it re-runs the
/// same reader path the acceptance scenario used and records what the archive
/// really holds, independently of any assertion about playback.
fn sound_census(game_dir: &Path) -> Census {
    use cs_assets::zbd::ZbdContainer;
    use cs_formats::ParseContext;

    let root = game_dir.to_path_buf();
    let found = discover(&root).expect("production discovery reads the installation");
    let context = cs_types::asset_id::ResolveContext::new(fingerprint(&found.manifest));
    let mut builder = cs_assets::vfs::SessionBuilder::new(context);
    builder
        .mount_installation(&root, &found.diagnosis)
        .expect("the installation mounts");
    let session = builder.open();

    let key = cs_types::asset_id::AssetKey::from_spelling("install", SOUND_CONTAINER, "default")
        .expect("the sound family's own container key is a valid key");
    let container =
        ZbdContainer::open(&session, &key).expect("the sound container routes and reads");
    let mut parse = ParseContext::with_defaults(container.label());
    let index = container.index(&mut parse).expect("its trailer indexes");
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .expect("its sound archive reads");

    let mut readiness: BTreeMap<String, usize> = BTreeMap::new();
    let mut decoded = 0usize;
    let mut first_decoded = None;
    let mut first_frames = 0u64;
    let mut first_rate_hz = 0u32;
    let mut first_channels = 0u16;
    for asset in assets.entries() {
        let code = match asset.readiness() {
            cs_assets::zbd::SoundReadiness::Decoded { .. } => "decoded".to_owned(),
            // An unsupported member keeps its **own** declared tag: a census
            // that folded every unsupported member into one bucket would hide
            // which formats this installation actually holds.
            cs_assets::zbd::SoundReadiness::UnsupportedFormat { tag, .. } => {
                format!("unsupported_format_{tag:#06x}")
            }
            cs_assets::zbd::SoundReadiness::UnreadableHeader { reason } => {
                format!("unreadable_header_{reason}")
            }
            cs_assets::zbd::SoundReadiness::Undecodable { code } => (*code).to_owned(),
        };
        *readiness.entry(code).or_default() += 1;
        if asset.readiness().is_decoded() {
            decoded += 1;
            if first_decoded.is_none() {
                let mut parse = ParseContext::with_defaults(container.label());
                let plan = cs_formats::zbd::SampleFormat::from_member(asset.content())
                    .expect("a decoded member's declaration is readable");
                first_frames =
                    cs_formats::zbd::decode_sound_sample(&mut parse, asset.content(), &plan)
                        .map(|decoded| decoded.sample_count())
                        .unwrap_or(0);
                first_rate_hz = plan.rate_hz();
                first_channels = plan.channels();
                first_decoded = Some(String::from_utf8_lossy(asset.name()).into_owned());
            }
        }
    }
    Census {
        total: assets.len(),
        decoded,
        readiness,
        first_decoded,
        first_frames,
        first_rate_hz,
        first_channels,
    }
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_lc_audio_device.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-LC-AUDIO-DEVICE` written relative
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

/// Extracts the libtest summaries and the per-test results of the
/// `accept_m01_lc_audio_device_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with(ACCEPTANCE_PREFIX) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
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
    (name, sha256(&bytes).to_hex(), kind.to_owned())
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
                 \"sound-member-census.json\"]}}",
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

fn str_array(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// The command line, each argument quoted as a JSON string.
fn str_array_owned(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

fn option_str(value: Option<&str>) -> String {
    match value {
        Some(value) => jstr(value),
        None => "null".to_owned(),
    }
}

/// A JSON string literal: quoted and escaped, so no report field can break out of
/// its string.
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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat` accepts
/// after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
