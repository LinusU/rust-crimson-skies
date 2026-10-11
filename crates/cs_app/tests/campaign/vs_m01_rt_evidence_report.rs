//! Acceptance stage VS-M01-RT-EVIDENCE: the private image, audio and trace
//! evidence of the M01 launch, written as `acceptance.json`
//! (Rally #1218; `docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`).
//!
//! The retail member runs the real `cs --mission M01` launch under
//! `CS_MISSION_CAPTURE=1`. The windowed composition itself writes the three
//! artifacts into `$CS_EVIDENCE_DIR`: the framebuffer PNG with the player
//! present and `trace.jsonl` (#1312), and `playback.json`, the audible
//! device's own record of what its output stream pulled from the original
//! engine loop (#1313). The harness checks each artifact, hashes it, and
//! writes `acceptance.json` from what the run produced; nothing is typed in
//! except the `unknowns` and `review` text, which state what they are.
//!
//! Run from the workspace root:
//!
//! ```sh
//! CS_EVIDENCE_DIR=private/evidence/VS-M01-RUNTIME \
//! CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!   cargo test --locked -p cs_app --test campaign -- accept_vs_m01_runtime_evidence_report --include-ignored
//! python3 tools/validate_evidence.py private/evidence/VS-M01-RUNTIME/acceptance.json \
//!   --artifact-root private/evidence/VS-M01-RUNTIME --require-pass
//! cp private/evidence/VS-M01-RUNTIME/acceptance.json docs/findings/evidence/VS-M01-RUNTIME.json
//! ```
//!
//! The argv and exit code recorded are those of the `cs` process the harness
//! itself spawned. A missing capability (`retail`, `gpu`, `audio`) or an
//! unset `CS_EVIDENCE_DIR` / `CS_CANDIDATE_TREE` fails nonzero and names it.
//! A GPU capture of our own run is not `human_play`, not a comparison with an
//! original screenshot, and nothing here is `verified_original`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::mission_session::{
    CAPABILITIES_ENV, CAPTURE_ENV, EVIDENCE_DIR_ENV, PLAYBACK_FILE, TRACE_FILE,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

const TASK_ID: &str = "VS-M01-RUNTIME";
const CANDIDATE_TREE_ENV: &str = "CS_CANDIDATE_TREE";

/// What the harness needs from its environment, or the capability it names as
/// missing.
fn require_environment(
    capabilities: Option<&str>,
    evidence_dir: Option<&str>,
    candidate_tree: Option<&str>,
    game_dir: Option<&str>,
) -> Result<(), String> {
    for capability in ["retail", "gpu", "audio"] {
        let has = capabilities.is_some_and(|list| {
            list.split(',')
                .any(|declared| declared.trim() == capability)
        });
        if !has {
            return Err(format!(
                "the M01 launch evidence needs the `{capability}` capability, but \
                 {CAPABILITIES_ENV} is {capabilities:?}"
            ));
        }
    }
    for (name, value) in [
        (EVIDENCE_DIR_ENV, evidence_dir),
        (CANDIDATE_TREE_ENV, candidate_tree),
        ("CS_GAME_DIR", game_dir),
    ] {
        if value.is_none_or(str::is_empty) {
            return Err(format!(
                "{name} is not set: the evidence run has nowhere to land"
            ));
        }
    }
    Ok(())
}

fn environment() -> (PathBuf, String, PathBuf) {
    let get = |name: &str| std::env::var(name).ok();
    if let Err(reason) = require_environment(
        get(CAPABILITIES_ENV).as_deref(),
        get(EVIDENCE_DIR_ENV).as_deref(),
        get(CANDIDATE_TREE_ENV).as_deref(),
        get("CS_GAME_DIR").as_deref(),
    ) {
        panic!("{reason}");
    }
    (
        workspace_path(&get(EVIDENCE_DIR_ENV).expect("checked")),
        get(CANDIDATE_TREE_ENV).expect("checked"),
        PathBuf::from(get("CS_GAME_DIR").expect("checked")),
    )
}

/// **A machine missing a capability, or a run without a directory, is refused
/// by name — never a silently skipped artifact.**
#[test]
fn accept_vs_m01_runtime_evidence_refuses_missing_capability_and_directory() {
    let ok = require_environment(Some("retail,gpu,audio"), Some("d"), Some("t"), Some("g"));
    assert_eq!(ok, Ok(()));
    for missing in ["retail", "gpu", "audio"] {
        let list: Vec<&str> = ["retail", "gpu", "audio"]
            .into_iter()
            .filter(|capability| *capability != missing)
            .collect();
        let error = require_environment(Some(&list.join(",")), Some("d"), Some("t"), Some("g"))
            .expect_err("a missing capability is refused");
        assert!(error.contains(&format!("`{missing}`")), "{error}");
    }
    let unset = require_environment(None, Some("d"), Some("t"), Some("g"))
        .expect_err("an unset capability list is refused");
    assert!(unset.contains(CAPABILITIES_ENV), "{unset}");
    for (dir, tree, game, name) in [
        (None, Some("t"), Some("g"), EVIDENCE_DIR_ENV),
        (Some("d"), Some(""), Some("g"), CANDIDATE_TREE_ENV),
        (Some("d"), Some("t"), None, "CS_GAME_DIR"),
    ] {
        let error = require_environment(Some("retail,gpu,audio"), dir, tree, game)
            .expect_err("an unset variable is refused");
        assert!(error.contains(name), "{error}");
    }
}

/// **The launch itself refuses a capture on a machine without `gpu`, with a
/// nonzero exit that names the capability.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_vs_m01_runtime_evidence_launch_without_gpu_exits_nonzero_naming_it() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is not set");
    let dir = std::env::temp_dir().join(format!("cs_m01_evidence_nogpu_{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--mission", "M01", "--cs-path", &game_dir])
        .env(CAPTURE_ENV, "1")
        .env(EVIDENCE_DIR_ENV, &dir)
        .env(CAPABILITIES_ENV, "retail,audio")
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "exit {:?}: {stderr}",
        output.status
    );
    assert!(stderr.contains("`gpu`"), "{stderr}");
    assert!(!dir.join(TRACE_FILE).exists(), "no artifact on a refusal");
    let _ = fs::remove_dir_all(&dir);
}

/// **The M01 launch writes the PNG, the trace and the playback record, and
/// the report that hashes them validates.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_vs_m01_runtime_evidence_report_writes_the_acceptance_report() {
    let (evidence_dir, candidate_tree, game_dir) = environment();
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit"
    );
    assert!(
        std::env::var("CS_ENGINE_IMAGE").is_ok(),
        "CS_ENGINE_IMAGE is not set: the M01 launch plan needs the decrypted image"
    );
    let _ = fs::remove_dir_all(&evidence_dir);
    fs::create_dir_all(&evidence_dir).expect("the evidence directory is created");

    let argv = [
        "cs".to_owned(),
        "--mission".to_owned(),
        "M01".to_owned(),
        "--cs-path".to_owned(),
        game_dir.display().to_string(),
    ];
    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(&argv[1..])
        .env(CAPTURE_ENV, "1")
        .env(EVIDENCE_DIR_ENV, &evidence_dir)
        .output()
        .expect("the cs binary runs");
    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(exit_code, 0, "the launch failed: {stderr}");

    // Image: a real framebuffer PNG with real content.
    let png = fs::read_dir(&evidence_dir)
        .expect("the evidence dir reads")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "png"))
        .expect("the launch wrote a framebuffer PNG");
    let decoded = image::open(&png).expect("the PNG decodes").to_luma8();
    let levels: std::collections::BTreeSet<u8> = decoded.pixels().map(|p| p.0[0]).collect();
    assert!(
        decoded.width() >= 640 && decoded.height() >= 360 && levels.len() >= 16,
        "{}x{} with {} luminance levels is not a rendered scene",
        decoded.width(),
        decoded.height(),
        levels.len()
    );

    // Trace: header first, terminal last, ticks/camera/input/screenshot between.
    let trace = fs::read_to_string(evidence_dir.join(TRACE_FILE)).expect("the trace was written");
    let kinds: Vec<&str> = trace
        .lines()
        .map(|line| {
            let start = line.find("\"kind\":\"").expect("every record has a kind") + 8;
            line[start..].split('"').next().expect("kind closes")
        })
        .collect();
    assert_eq!(kinds.first(), Some(&"header"));
    assert_eq!(kinds.last(), Some(&"terminal"));
    for kind in ["tick", "camera", "input", "screenshot"] {
        assert!(kinds.contains(&kind), "no {kind} record in the trace");
    }

    // Audio: the device's own playback record, its own artifact.
    let playback =
        fs::read_to_string(evidence_dir.join(PLAYBACK_FILE)).expect("the playback was written");
    for needle in [
        "\"boundary\": \"sample_probe_source_pull\"",
        "\"backend\": \"audible\"",
        "\"member\": \"devastator.wav\"",
        "\"verified_original\": false",
    ] {
        assert!(playback.contains(needle), "{needle} missing: {playback}");
    }
    let frames_pulled: u64 = playback
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"frames_pulled\": "))
        .and_then(|value| value.trim_end_matches(',').parse().ok())
        .expect("the record counts the frames pulled");
    assert!(frames_pulled > 0, "{playback}");

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let png_name = png
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let artifacts = [
        (png_name.clone(), "image"),
        (TRACE_FILE.to_owned(), "trace"),
        (PLAYBACK_FILE.to_owned(), "audio"),
    ]
    .map(|(name, kind)| {
        let bytes = fs::read(evidence_dir.join(&name)).expect("artifact reads");
        format!(
            "{{\"path\": {}, \"sha256\": \"{}\", \"kind\": \"{kind}\"}}",
            jstr(&name),
            sha256(&bytes).to_hex()
        )
    })
    .join(", ");
    let assertion = |id: &str, evidence: &str| {
        format!(
            "{{\"id\": {}, \"status\": \"pass\", \"evidence\": [{}]}}",
            jstr(id),
            jstr(evidence)
        )
    };
    let assertions = [
        assertion(
            "image: a framebuffer PNG of the M01 world with the player present is >= 640x360 with >= 16 luminance levels",
            &png_name,
        ),
        assertion(
            "trace: header, tick, camera, input and screenshot records end in a terminal record",
            TRACE_FILE,
        ),
        assertion(
            "audio: the audible AudioDevice backend pulled frames and non-silent samples of the original devastator.wav engine loop",
            PLAYBACK_FILE,
        ),
        assertion("launch: the cs process exited 0", TRACE_FILE),
    ]
    .join(", ");

    let argv_json = format!(
        "[{}]",
        argv.iter().map(|a| jstr(a)).collect::<Vec<_>>().join(", ")
    );
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": {task},\n \"candidate_tree\": {tree},\n \
         \"engine\": {{\"rust\": {rust}, \"bevy\": {bevy}, \"avian\": {avian}}},\n \
         \"created_at\": {at},\n \
         \"command\": {{\"argv\": {argv}, \"cwd\": {cwd}, \"exit_code\": {exit_code}}},\n \
         \"source\": {{\"install_sha256\": \"{install}\", \"content_sha256\": \"{content}\"}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [{overrides}],\n \
         \"capabilities\": [\"retail\", \"gpu\", \"audio\"],\n \
         \"tests\": {{\"discovered\": 1, \"executed\": 1, \"passed\": 1, \"failed\": 0, \"ignored\": 0}},\n \
         \"assertions\": [{assertions}],\n \"artifacts\": [{artifacts}],\n \
         \"unknowns\": [],\n \
         \"review\": {{\"identity\": {identity}, \"method\": {method}}},\n \"claim\": \"implemented\"\n}}\n",
        task = jstr(TASK_ID),
        tree = jstr(&candidate_tree),
        rust = jstr(&rustc_version()),
        bevy = jstr(&locked_version("bevy")),
        avian = jstr(&locked_version("avian3d")),
        at = jstr(&iso_utc_now()),
        argv = argv_json,
        cwd = jstr(&git(&["rev-parse", "--show-toplevel"])),
        install = install_sha256,
        content = content_sha256,
        overrides = jstr("scripted input: hold S (pitch up) for one second of rendered frames"),
        identity = jstr(
            "implementer: sonnet-1 (Rally #1218 implement claim of 2026-10-12); reviewer: recorded \
             by the Rally review claim that follows. An agent review is `checked` at best and never \
             replaces the owner's human approval.",
        ),
        method = jstr(
            "NOT PROVEN, by design of this slice: this is an automated run of our own build with a \
             scripted pitch-up, so it is not human_play; the audio record is the source boundary \
             (frames the output stream pulled), not a speaker capture, and the image was not \
             compared with an original screenshot, so neither is human_review; nothing here is \
             verified_original (claim: implemented). The sound archive the original opens for the \
             engine loop is unmeasured (playback.json labels it a choice; see \
             docs/findings/2026-10-11-vs-m01-rt-evidence-audio-binding.md), and the owner run \
             #358 REF-OWNER-FIRST-CAPTURE is what resolves it. Method: the harness spawned the real `cs --mission M01` launch with CS_MISSION_CAPTURE=1 on a \
             machine with the retail, gpu and audio capabilities; the windowed composition wrote \
             the PNG, trace.jsonl and playback.json; this harness checked them and hashed them. \
             Structure checked with tools/validate_evidence.py; the single recorded test is this \
             harness. validated with --require-pass.",
        ),
    );
    fs::write(evidence_dir.join("acceptance.json"), &report).expect("the report is written");
    println!("wrote {}", evidence_dir.join("acceptance.json").display());
}

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
        "git {args:?} failed: {}",
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

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
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

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        (month_prime - 10) / 12 + 1
    }) as u32;
    let year = if month <= 2 {
        year_of_day - 1
    } else {
        year_of_day
    };
    let second = (rest % 60) as u32;
    let minute = ((rest / 60) % 60) as u32;
    let hour = (rest / 3600) as u32;
    (year, month, day, hour, minute, second)
}
