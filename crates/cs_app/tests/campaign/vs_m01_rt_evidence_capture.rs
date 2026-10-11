//! Acceptance stage VS-M01-RT-EVIDENCE-A: the windowed composition's capture
//! path (Rally #1312).
//!
//! `cs --mission M01 --cs-path <dir>` with `CS_MISSION_CAPTURE=1` writes a
//! real framebuffer PNG with the player present and a `trace.jsonl` of the
//! run into `CS_EVIDENCE_DIR`. The synthetic members prove the refusals: no
//! `gpu` in `CS_CAPABILITIES` and no capture directory are named, never
//! skipped. The retail member needs a window, so it is
//! `#[ignore = "requires CS_GAME_DIR"]` and runs under `--include-ignored`
//! with `CS_CAPABILITIES` listing `gpu` and `CS_ENGINE_IMAGE` set. Nothing
//! captured is committed, and no `acceptance.json` is written here (#1218).

use std::path::PathBuf;
use std::process::Command;

use cs_app::mission_session::{
    CAPABILITIES_ENV, CaptureRefusal, CaptureSettings, EVIDENCE_DIR_ENV, MissionCapturePlugin,
    TRACE_FILE, finish_capture,
};

/// **A capture without `gpu` or without a directory is refused by name.**
#[test]
fn accept_vs_m01_runtime_evidence_capture_refuses_missing_gpu_and_directory() {
    let missing = CaptureSettings::resolve(Some("retail,audio"), Some("/tmp/x"))
        .expect_err("no gpu is refused");
    assert!(matches!(missing, CaptureRefusal::MissingCapability { .. }));
    let text = missing.to_string();
    assert!(
        text.contains("gpu") && text.contains(CAPABILITIES_ENV),
        "{text}"
    );
    let unset = CaptureSettings::resolve(None, Some("/tmp/x")).expect_err("unset is refused");
    assert!(unset.to_string().contains("unset"), "{unset}");

    let no_dir = CaptureSettings::resolve(Some("retail, gpu"), None).expect_err("no dir");
    assert_eq!(no_dir, CaptureRefusal::MissingDirectory);
    assert!(no_dir.to_string().contains(EVIDENCE_DIR_ENV), "{no_dir}");
    assert_eq!(
        CaptureSettings::resolve(Some("gpu"), Some("")).expect_err("empty dir"),
        CaptureRefusal::MissingDirectory
    );

    let ok = CaptureSettings::resolve(Some("retail,gpu,audio"), Some("/tmp/x")).expect("ok");
    assert_eq!(ok.dir, PathBuf::from("/tmp/x"));
}

/// **A run that never stepped the host or got a frame produces no evidence
/// and says so — after writing the trace that shows why.**
#[test]
fn accept_vs_m01_runtime_evidence_capture_finish_refuses_an_uncaptured_run() {
    let dir = std::env::temp_dir().join(format!("cs_m01_capture_refusal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = CaptureSettings::resolve(Some("gpu"), dir.to_str()).expect("resolves");
    let plugin = MissionCapturePlugin::new(settings, "synthetic");
    let error = finish_capture(&plugin.handle()).expect_err("no tick and no frame, no evidence");
    assert!(matches!(error, CaptureRefusal::Incomplete(_)), "{error}");
    let trace = std::fs::read_to_string(dir.join(TRACE_FILE)).expect("the trace survives");
    assert!(trace.contains("\"kind\":\"header\""), "{trace}");
    assert!(trace.contains("\"settled\":false"), "{trace}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The retail launch writes a real PNG and a trace.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_vs_m01_runtime_evidence_capture_writes_png_and_trace_from_the_launch() {
    let game_dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: run with --include-ignored and the installation")
    });
    assert!(
        std::env::var("CS_ENGINE_IMAGE").is_ok(),
        "CS_ENGINE_IMAGE is not set: the M01 launch plan needs the decrypted image"
    );
    let capabilities = std::env::var(CAPABILITIES_ENV).unwrap_or_default();
    assert!(
        capabilities.split(',').any(|c| c.trim() == "gpu"),
        "this test needs the gpu capability in {CAPABILITIES_ENV}, got {capabilities:?}"
    );
    let dir = std::env::temp_dir().join(format!("cs_m01_evidence_capture_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--mission", "M01", "--cs-path", &game_dir])
        .env("CS_MISSION_CAPTURE", "1")
        .env(EVIDENCE_DIR_ENV, &dir)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cs exited {:?}: {stderr}",
        output.status
    );

    let png = std::fs::read_dir(&dir)
        .expect("the capture dir exists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "png"))
        .expect("a framebuffer PNG was written");
    let decoded = image::open(&png).expect("the PNG decodes").to_luma8();
    assert!(decoded.width() >= 640 && decoded.height() >= 360);
    let levels: std::collections::BTreeSet<u8> = decoded.pixels().map(|p| p.0[0]).collect();
    assert!(levels.len() >= 16, "only {} luminance levels", levels.len());

    let trace = std::fs::read_to_string(dir.join(TRACE_FILE)).expect("the trace was written");
    let kinds: Vec<&str> = trace
        .lines()
        .map(|line| {
            let start = line.find("\"kind\":\"").expect("every record has a kind") + 8;
            line[start..].split('"').next().expect("kind closes")
        })
        .collect();
    assert_eq!(kinds.first(), Some(&"header"));
    for kind in ["tick", "camera", "input", "screenshot"] {
        assert!(kinds.contains(&kind), "no {kind} record in {kinds:?}");
    }
    assert_eq!(kinds.last(), Some(&"terminal"));
    let _ = std::fs::remove_dir_all(&dir);
}
