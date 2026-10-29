//! F52-W2-evidence (#2141) acceptance tests.
//! Prefix: `accept_f52_w2_evidence_`.
//!
//! The visible and audible claims are judged from the mission smoke's own
//! `presentation` records — what each delivered frame presented when its
//! screenshot was requested. The headless tests drive the real check judge
//! ([`evaluate_mission_smoke`]) and the real serializer
//! ([`presentation_json`]); the windowed half that produces the framebuffer
//! PNGs and the voiced probe is the ignored test at the bottom, run with
//! `--include-ignored` under `CS_GAME_DIR` + `gpu` + `audio`.

use std::path::PathBuf;

use cs_app::mission_session::{
    Check, PresentationMetrics, SMOKE_KIND, SMOKE_REPORT_FILE, ShotPresentation, SmokeMeasurements,
    evaluate_mission_smoke, presentation_json,
};

/// The presentation records a healthy scripted run leaves in `report.json`:
/// the labelled probe line on the production radio queue, two designed
/// `PlayerHit`s, and the phase's mid-run settings flips — filter off then
/// monochrome, cosmetic motion kept then reduced, the subtitle row drawn
/// then suppressed, UI scale 100% then 150% — each frame's record naming
/// the state it presented. Shared with `accept_m01_owner_smoke`'s healthy
/// mission, because a real scripted run always carries them.
pub fn healthy_presentation() -> PresentationMetrics {
    let caption = "smoke-probe: presentation probe (designed test line)";
    let queue_caption = "presentation probe (designed test line)";
    PresentationMetrics {
        probe_speaker: Some("smoke-probe".to_owned()),
        probe_subtitle: Some(queue_caption.to_owned()),
        probe_asset: Some("i_m01_dlg_001".to_owned()),
        probe_event: Some("player_hit".to_owned()),
        probe_refusals: vec![],
        shots: vec![
            // A pre-phase shot records the run's own session: defaults here.
            ShotPresentation {
                label: "hud".to_owned(),
                frame: 900,
                colour_filter: "off".to_owned(),
                ui_scale_percent: 100,
                subtitles_on: false,
                reduce_shake: false,
                reduce_flash: false,
                text_px: 16,
                subtitle: None,
                flash: 0.0,
                shake: 0.0,
                presented: vec![],
                radio_speaker: Some("Wingman".to_owned()),
                radio_subtitle: None,
                probe_active: false,
            },
            // The designed hit, presented whole under the open settings.
            ShotPresentation {
                label: "presentation_probe_fx".to_owned(),
                frame: 1_051,
                colour_filter: "off".to_owned(),
                ui_scale_percent: 100,
                subtitles_on: true,
                reduce_shake: false,
                reduce_flash: false,
                text_px: 16,
                subtitle: Some(caption.to_owned()),
                flash: 0.5,
                shake: 0.75,
                presented: vec![
                    "camera_shake".to_owned(),
                    "screen_flash".to_owned(),
                    "damage_notice".to_owned(),
                ],
                radio_speaker: Some("smoke-probe".to_owned()),
                radio_subtitle: Some(queue_caption.to_owned()),
                probe_active: true,
            },
            // The same designed hit under the reduced-motion pair: only the
            // required notice presents, on a filtered, larger-text frame.
            ShotPresentation {
                label: "presentation_probe_fx_reduced".to_owned(),
                frame: 1_054,
                colour_filter: "monochrome".to_owned(),
                ui_scale_percent: 150,
                subtitles_on: true,
                reduce_shake: true,
                reduce_flash: true,
                text_px: 24,
                subtitle: Some(caption.to_owned()),
                flash: 0.0,
                shake: 0.0,
                presented: vec!["damage_notice".to_owned()],
                radio_speaker: Some("smoke-probe".to_owned()),
                radio_subtitle: Some(queue_caption.to_owned()),
                probe_active: true,
            },
            // The subtitle shot: the captioned probe line still active.
            ShotPresentation {
                label: "presentation_subtitle".to_owned(),
                frame: 1_080,
                colour_filter: "monochrome".to_owned(),
                ui_scale_percent: 150,
                subtitles_on: true,
                reduce_shake: true,
                reduce_flash: true,
                text_px: 24,
                subtitle: Some(caption.to_owned()),
                flash: 0.0,
                shake: 0.0,
                presented: vec![],
                radio_speaker: Some("smoke-probe".to_owned()),
                radio_subtitle: Some(queue_caption.to_owned()),
                probe_active: true,
            },
            // The switch off: the row is undrawn while the queue's active
            // line still carries the caption.
            ShotPresentation {
                label: "presentation_subtitle_off".to_owned(),
                frame: 1_082,
                colour_filter: "monochrome".to_owned(),
                ui_scale_percent: 150,
                subtitles_on: false,
                reduce_shake: true,
                reduce_flash: true,
                text_px: 24,
                subtitle: None,
                flash: 0.0,
                shake: 0.0,
                presented: vec![],
                radio_speaker: Some("smoke-probe".to_owned()),
                radio_subtitle: Some(queue_caption.to_owned()),
                probe_active: true,
            },
            // A post-phase shot records the restored session.
            ShotPresentation {
                label: "guns".to_owned(),
                frame: 1_150,
                colour_filter: "off".to_owned(),
                ui_scale_percent: 100,
                subtitles_on: false,
                reduce_shake: false,
                reduce_flash: false,
                text_px: 16,
                subtitle: None,
                flash: 0.0,
                shake: 0.0,
                presented: vec![],
                radio_speaker: None,
                radio_subtitle: None,
                probe_active: false,
            },
        ],
    }
}

/// The check judge's own verdict: the applicable checks that failed.
fn failures(checks: &[Check]) -> Vec<&str> {
    checks
        .iter()
        .filter(|check| check.applies && !check.passed)
        .map(|check| check.id.as_str())
        .collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the smoke reports a {id} check: {checks:?}"))
}

/// The probe checks produced for one measurements record.
fn probe_checks(m: &SmokeMeasurements) -> Vec<Check> {
    evaluate_mission_smoke(m)
        .into_iter()
        .filter(|check| check.id.starts_with("presentation_probe_"))
        .collect()
}

fn measurements(presentation: PresentationMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        mission: "M01".to_owned(),
        presentation,
        ..SmokeMeasurements::default()
    }
}

/// A healthy probe run passes every presentation check the smoke makes —
/// the paired states each claim needs sit on delivered frames.
#[test]
fn accept_f52_w2_evidence_the_judge_passes_a_healthy_probe_run() {
    let checks = probe_checks(&measurements(healthy_presentation()));
    assert_eq!(
        checks.len(),
        8,
        "the presentation probe has eight checks: {checks:?}"
    );
    for verdict in &checks {
        assert!(
            verdict.applies,
            "{} applies: {}",
            verdict.id, verdict.detail
        );
        assert!(verdict.passed, "{}: {}", verdict.id, verdict.detail);
    }
    for id in [
        "presentation_probe_radio",
        "presentation_probe_event",
        "presentation_probe_active",
        "presentation_probe_subtitle",
        "presentation_probe_effects",
        "presentation_probe_filter",
        "presentation_probe_scale",
        "presentation_probe_settings",
    ] {
        check(&checks, id);
    }
}

/// A run that never reached the phase produced no probe records: its
/// checks are recorded as not applying rather than failed.
#[test]
fn accept_f52_w2_evidence_the_judge_does_not_apply_without_the_probe() {
    let checks = probe_checks(&measurements(PresentationMetrics::default()));
    assert!(
        checks.iter().all(|check| !check.applies),
        "no probe records, no applicable checks: {checks:?}"
    );
    assert!(
        failures(&checks).is_empty(),
        "a run without the probe records no presentation failures: {:?}",
        failures(&checks)
    );
}

/// A phase that ran but could not inject — no audio session, no event
/// queue — records its refusal and fails by name, never silently passes.
#[test]
fn accept_f52_w2_evidence_the_judge_fails_a_probe_that_refused() {
    let checks = probe_checks(&measurements(PresentationMetrics {
        probe_refusals: vec!["no audio session".to_owned()],
        ..PresentationMetrics::default()
    }));
    for id in ["presentation_probe_radio", "presentation_probe_event"] {
        let verdict = check(&checks, id);
        assert!(verdict.applies, "{id} applies to a refused phase");
        assert!(!verdict.passed, "{id} fails: {}", verdict.detail);
    }
}

/// A frame whose presented effects disagree with its recorded switches —
/// shake listed under reduced motion — fails the effects check.
#[test]
fn accept_f52_w2_evidence_the_judge_fails_inconsistent_effect_records() {
    let mut presentation = healthy_presentation();
    for shot in &mut presentation.shots {
        if shot.label == "presentation_probe_fx_reduced" {
            // The recorded frame says reduced motion yet carried the shake —
            // the exact inconsistency the check exists to catch.
            shot.presented.push("camera_shake".to_owned());
            shot.shake = 0.5;
        }
    }
    let checks = probe_checks(&measurements(presentation));
    let verdict = check(&checks, "presentation_probe_effects");
    assert!(verdict.applies);
    assert!(
        !verdict.passed,
        "a reduced frame presenting shake fails: {}",
        verdict.detail
    );
}

/// A captioned line whose row stayed drawn after the switch went off fails
/// the subtitle check — the suppressed state is evidence too.
#[test]
fn accept_f52_w2_evidence_the_judge_fails_a_row_the_switch_did_not_suppress() {
    let mut presentation = healthy_presentation();
    for shot in &mut presentation.shots {
        if shot.label == "presentation_subtitle_off" {
            shot.subtitle = Some("smoke-probe: presentation probe".to_owned());
        }
    }
    let checks = probe_checks(&measurements(presentation));
    let verdict = check(&checks, "presentation_probe_subtitle");
    assert!(verdict.applies);
    assert!(
        !verdict.passed,
        "a row drawn under the off switch fails: {}",
        verdict.detail
    );
}

/// The report's `presentation` block is the run's own JSON: a real parser
/// reads back the probe and every shot record, escapes and all.
#[test]
fn accept_f52_w2_evidence_presentation_json_round_trips_the_records() {
    let json = presentation_json(&healthy_presentation());
    let value: serde_json::Value =
        serde_json::from_str(&json).expect("the presentation block is real JSON");
    assert_eq!(value["probe_speaker"], "smoke-probe");
    assert_eq!(value["probe_asset"], "i_m01_dlg_001");
    assert_eq!(value["probe_event"], "player_hit");
    assert_eq!(value["probe_refusals"], serde_json::json!([]));
    let shots = value["shots"].as_array().expect("shots is an array");
    assert_eq!(shots.len(), 6);
    let fx = shots
        .iter()
        .find(|shot| shot["label"] == "presentation_probe_fx")
        .expect("the all-effects shot is listed");
    assert_eq!(fx["colour_filter"], "off");
    assert_eq!(fx["subtitles_on"], true);
    assert_eq!(fx["probe_active"], true);
    assert_eq!(fx["radio_speaker"], "smoke-probe");
    assert_eq!(
        fx["presented"]
            .as_array()
            .expect("presented names the effects")[2],
        "damage_notice"
    );
    let off = shots
        .iter()
        .find(|shot| shot["label"] == "presentation_subtitle_off")
        .expect("the subtitles-off shot is listed");
    assert_eq!(off["subtitles_on"], false);
    assert_eq!(off["subtitle"], serde_json::Value::Null);
    assert_eq!(
        off["radio_subtitle"], "presentation probe (designed test line)",
        "the queue's caption is the suppressed row's provenance"
    );
}

/// The windowed binary the ignored run drives: `CS_MISSION_BIN` when the
/// operator names one, else the release build (the documented way to run a
/// windowed smoke — debug Bevy steps far too slowly), else the test
/// harness's own binary.
fn mission_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_MISSION_BIN") {
        return PathBuf::from(bin);
    }
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/cs");
    if release.is_file() {
        return release;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run, on the owner's own command, proves the presented
/// claims.**
///
/// This is the half that needs the installation, the GPU and the audio
/// device: it runs the exact launch the owner plays with the smoke flags,
/// and asserts the evidence contract — every `presentation_probe_*` check
/// is named and passed, the four probe framebuffer PNGs arrive, the
/// per-shot records carry both states of every switch, and the designed
/// radio line's voiced start is in `audio.one_shots`.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_f52_w2_evidence_the_windowed_launch_proves_the_presented_claims() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the mission launch needs gpu and audio");
    };
    for capability in ["gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|part| part.trim() == capability),
            "CS_CAPABILITIES must list {capability}: {capabilities}"
        );
    }
    let engine_image = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE names the owner's decrypted executable");
    let dir = std::env::var("CS_F52_W2_EVIDENCE_OUT").map_or_else(
        |_| std::env::temp_dir().join("f52-w2-evidence-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let profile_dir = dir.join("profile");
    std::fs::create_dir_all(&profile_dir).expect("the private profile directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .arg("--profile-dir")
        .arg(&profile_dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{} is written: {error}\n{stderr}", SMOKE_REPORT_FILE));
    assert!(
        report.contains(&format!("\"kind\":\"{SMOKE_KIND}\"")),
        "the report names its kind: {report}"
    );
    assert!(
        report.contains("\"human_play\":false"),
        "a scripted run is never human_play: {report}"
    );
    for id in [
        "presentation_probe_radio",
        "presentation_probe_event",
        "presentation_probe_active",
        "presentation_probe_subtitle",
        "presentation_probe_effects",
        "presentation_probe_filter",
        "presentation_probe_scale",
        "presentation_probe_settings",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"{id}\"")),
            "the report judges {id}: {report}"
        );
    }
    let passed = report
        .rfind("\"passed\":")
        .is_some_and(|index| report[index..].starts_with("\"passed\":true"));
    assert!(
        report.contains("\"failures\":[]") == passed,
        "`passed` is exactly \"no failures\": {report}"
    );

    // The probe block is the run's own JSON, parsed back.
    let report_json: serde_json::Value =
        serde_json::from_str(&report).expect("report.json is real JSON");
    let presentation = &report_json["presentation"];
    assert_eq!(presentation["probe_speaker"], "smoke-probe");
    assert_eq!(presentation["probe_event"], "player_hit");
    assert_eq!(
        presentation["probe_refusals"],
        serde_json::json!([]),
        "the probe injected on the production queues without refusal: {report}"
    );
    let probe_asset = presentation["probe_asset"]
        .as_str()
        .expect("the labelled test line voiced a delivered member");
    let shots = presentation["shots"]
        .as_array()
        .expect("per-shot presentation records");
    for (label, field, want) in [
        ("presentation_probe_fx", "reduce_shake", false),
        ("presentation_probe_fx", "colour_filter", false),
        ("presentation_probe_fx_reduced", "reduce_shake", true),
        ("presentation_probe_fx_reduced", "colour_filter", true),
        ("presentation_subtitle", "subtitles_on", true),
        ("presentation_subtitle_off", "subtitles_on", false),
    ] {
        let shot = shots
            .iter()
            .find(|shot| shot["label"] == label)
            .unwrap_or_else(|| panic!("the {label} record exists: {report}"));
        match field {
            "colour_filter" => assert_eq!(
                shot["colour_filter"] == "monochrome",
                want,
                "{label} carried the filter state it names: {report}"
            ),
            _ => assert_eq!(
                shot[field], want,
                "{label} recorded {field} = {want}: {report}"
            ),
        }
    }
    // Every probe shot was of a delivered frame: the PNGs arrived.
    for label in [
        "presentation_probe_fx",
        "presentation_probe_fx_reduced",
        "presentation_subtitle",
        "presentation_subtitle_off",
    ] {
        let png = std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("_{label}.png"))
            });
        assert!(png, "the frame_*_{label}.png framebuffer is in {:?}", dir);
    }
    // The audio path: the designed line's voiced start is one of the
    // one-shots the device tap recorded, post-bus-level.
    let one_shots = report_json["audio"]["one_shots"]
        .as_array()
        .expect("audio.one_shots lists the voiced starts");
    assert!(
        one_shots
            .iter()
            .any(|row| row["member"] == probe_asset && row["refused"].is_null()),
        "the probe's voiced start ({probe_asset}) is in audio.one_shots: {report}"
    );
    assert_eq!(
        output.status.success(),
        passed,
        "the process exits non-zero exactly when a check failed; stderr: {stderr}"
    );
    println!(
        "F52-W2 evidence smoke artifacts in {}: report passed = {passed}",
        dir.display()
    );
}
