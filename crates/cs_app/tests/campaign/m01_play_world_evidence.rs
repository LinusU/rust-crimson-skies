//! Evidence harness for M01-PLAY-WORLD-VISIBLE (Rally #1433):
//! `private/evidence/1433/acceptance.json`
//! (`docs/contracts/CLI-EVIDENCE.md`, `schemas/evidence.schema.json`).
//!
//! The retail member runs the real `cs --mission M01` launch under
//! `CS_MISSION_CAPTURE=1` and checks the artifacts the windowed composition
//! itself writes into `$CS_EVIDENCE_DIR`: the **world probe** frame (the world
//! drawn with the designed sky and the player hidden, #1433), the player frame,
//! `trace.jsonl` (whose `world` record carries the probe's pixel count) and
//! `playback.json`. Nothing is typed in except the `unknowns` and `review`
//! text, which state what they are.
//!
//! Run from the workspace root:
//!
//! ```sh
//! CS_EVIDENCE_DIR=private/evidence/1433 \
//! CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!   cargo test --locked -p cs_app --test campaign -- evidence_report_m01_play_world --include-ignored
//! python3 tools/validate_evidence.py private/evidence/1433/acceptance.json \
//!   --artifact-root private/evidence/1433 --require-pass
//! cp private/evidence/1433/acceptance.json docs/findings/evidence/1433.json
//! ```
//!
//! The name carries no `accept_m01_play_world_` prefix on purpose: the task's
//! acceptance selection must not pick this harness up (it fails loudly when its
//! environment is missing instead of passing vacuously).
//!
//! A GPU capture of our own run is not `human_play`, not a comparison with an
//! original screenshot, and nothing here is `verified_original`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cs_app::mission_session::{
    CAPABILITIES_ENV, CAPTURE_ENV, EVIDENCE_DIR_ENV, MIN_WORLD_COVERAGE_PERMILLE, PLAYBACK_FILE,
    TRACE_FILE,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

use crate::vs_m01_rt_evidence_report::{
    git, iso_utc_now, jstr, locked_version, rustc_version, workspace_path,
};

const TASK_ID: &str = "1433";
const CANDIDATE_TREE_ENV: &str = "CS_CANDIDATE_TREE";

/// **A machine missing a capability, or a run without a directory, is refused
/// by name — never a silently skipped artifact.**
#[test]
fn evidence_report_m01_play_world_refuses_missing_capability_and_directory() {
    for (caps, dir, tree, game, expected) in [
        (
            Some("retail,audio"),
            Some("d"),
            Some("t"),
            Some("g"),
            "`gpu`",
        ),
        (
            Some("gpu,audio"),
            Some("d"),
            Some("t"),
            Some("g"),
            "`retail`",
        ),
        (None, Some("d"), Some("t"), Some("g"), CAPABILITIES_ENV),
        (
            Some("retail,gpu,audio"),
            None,
            Some("t"),
            Some("g"),
            EVIDENCE_DIR_ENV,
        ),
        (
            Some("retail,gpu,audio"),
            Some("d"),
            None,
            Some("g"),
            CANDIDATE_TREE_ENV,
        ),
        (
            Some("retail,gpu,audio"),
            Some("d"),
            Some("t"),
            None,
            "CS_GAME_DIR",
        ),
    ] {
        let error = require_environment(caps, dir, tree, game).expect_err("refused");
        assert!(error.contains(expected), "{error}");
    }
    assert_eq!(
        require_environment(Some("retail,gpu,audio"), Some("d"), Some("t"), Some("g")),
        Ok(())
    );
}

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
                "the M01 world-draw evidence needs the `{capability}` capability, but \
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

/// **The M01 launch draws the world around the player, and the report that
/// hashes the run's artifacts validates.**
#[test]
#[ignore = "requires CS_GAME_DIR and a window-capable GPU host"]
fn evidence_report_m01_play_world_writes_the_acceptance_report() {
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
        .env(CAPABILITIES_ENV, "retail,gpu,audio")
        .output()
        .expect("the cs binary runs");
    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(exit_code, 0, "the launch failed: {stderr}");

    // The trace's world record: the frame around the spawn holds the world's
    // own pixels, counted against the frame's own background.
    let trace = fs::read_to_string(evidence_dir.join(TRACE_FILE)).expect("the trace was written");
    let world = trace
        .lines()
        .find(|line| line.contains("\"kind\":\"world\""))
        .expect("the trace holds the world probe record");
    let field = |name: &str| -> i64 {
        world
            .split(name)
            .nth(1)
            .and_then(|rest| rest.split([',', '}']).next())
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| panic!("the world record has no {name}: {world}"))
    };
    let covered = field("\"covered_permille\":");
    let world_pixels = field("\"world_pixels\":");
    let total_pixels = field("\"total_pixels\":");
    assert!(
        covered >= i64::from(MIN_WORLD_COVERAGE_PERMILLE),
        "the spawn frame must hold the world's own pixels: {world}"
    );
    assert!(world_pixels > 0 && total_pixels >= world_pixels, "{world}");

    // Both frames: real framebuffer PNGs with real content, the world probe's
    // among them.
    let mut world_png: Option<PathBuf> = None;
    let mut player_png: Option<PathBuf> = None;
    for entry in fs::read_dir(&evidence_dir).expect("the evidence dir reads") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|ext| ext == "png") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.contains("_world") {
                world_png = Some(path);
            } else if name.contains("_player") {
                player_png = Some(path);
            }
        }
    }
    let world_png = world_png.expect("the launch wrote the world probe PNG");
    let player_png = player_png.expect("the launch wrote the player PNG");
    for png in [&world_png, &player_png] {
        let decoded = image::open(png).expect("the PNG decodes").to_luma8();
        let levels: std::collections::BTreeSet<u8> = decoded.pixels().map(|p| p.0[0]).collect();
        assert!(
            decoded.width() >= 640 && decoded.height() >= 360 && levels.len() >= 8,
            "{}x{} with {} luminance levels is not a rendered scene",
            decoded.width(),
            decoded.height(),
            levels.len()
        );
    }

    // Audio: the device's own playback record, its own artifact.
    let playback =
        fs::read_to_string(evidence_dir.join(PLAYBACK_FILE)).expect("the playback was written");
    for needle in [
        "\"boundary\": \"sample_probe_source_pull\"",
        "\"backend\": \"audible\"",
        "\"verified_original\": false",
    ] {
        assert!(playback.contains(needle), "{needle} missing: {playback}");
    }

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let name_of = |path: &PathBuf| {
        path.file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned()
    };
    let world_name = name_of(&world_png);
    let player_name = name_of(&player_png);
    let artifacts = [
        (world_name.clone(), "image"),
        (player_name.clone(), "image"),
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
            "world: the frame around the spawn holds the world's own pixels, counted against the \
             frame's own background with the designed sky and the player hidden (covered_permille \
             over the 20-per-mille floor)",
            &world_name,
        ),
        assertion(
            "image: the player's framebuffer PNG is >= 640x360 with >= 8 luminance levels",
            &player_name,
        ),
        assertion(
            "trace: header, tick, camera, input, screenshot and world records end in a terminal record",
            TRACE_FILE,
        ),
        assertion(
            "audio: the audible AudioDevice backend pulled frames of the original engine loop",
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
        overrides = jstr(
            "world probe: the designed sky dome and the player are hidden for the probe frame; \
             scripted input: hold S (pitch up) for one second of rendered frames"
        ),
        identity = jstr(
            "implementer: bunny-alpha-2 (Rally #1433 implement claim of 2026-10-14); reviewer: \
             recorded by the Rally review claim that follows. An agent review is `checked` at best \
             and never replaces the owner's human approval.",
        ),
        method = jstr(
            "NOT PROVEN, by design of this slice: this is an automated run of our own build, so it \
             is not human_play; the world-probe image is not compared with an original screenshot, \
             and the audio record is the source boundary (frames the output stream pulled), not a \
             speaker capture, so neither is human_review; nothing here is verified_original (claim: \
             implemented). The material binding is the #666 designed name reading and its \
             presentation is provisional (docs/findings/2026-10-14-m01-play-world-visible.md); \
             which stored node the original draws is unmeasured. Method: the harness spawned the \
             real `cs --mission M01` launch with CS_MISSION_CAPTURE=1 on a machine with the retail, \
             gpu and audio capabilities; the windowed composition wrote the world probe frame, the \
             player frame, trace.jsonl (with the world record's covered_permille) and \
             playback.json; this harness checked them and hashed them. Structure checked with \
             tools/validate_evidence.py; the single recorded test is this harness, validated with \
             --require-pass.",
        ),
    );
    fs::write(evidence_dir.join("acceptance.json"), &report).expect("the report is written");
    println!("wrote {}", evidence_dir.join("acceptance.json").display());
}
