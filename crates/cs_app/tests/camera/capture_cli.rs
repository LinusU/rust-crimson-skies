//! F21-E acceptance tests: the deterministic capture flags parse into a
//! `camera::CaptureRequest`, and the capture run reports the applied
//! `CaptureReport`.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, non-negotiable
//! behavior 5: "Screenshot CLI accepts a reproducible world pose, mission id,
//! tick and deterministic settings; report all overrides." The flags are
//! `--mission <id> --tick <n> --pose <x,y,z> [--aspect <w:h>]
//! [--view cockpit|chase|spyglass] [--settings fixed]` inside the
//! `--synthetic --headless` run mode, because a capture of "the game" is not
//! reproducible and the SYNTHETIC scene is the only world this workspace can
//! honestly run without an installation or a GPU.
//!
//! Every test calls production code: `cs_app::cli::parse` for the flag
//! grammar, `cs_app::run::run_capture` for the applied report, and the real
//! `cs` binary for the printed one. Removing the flag arms, the capture
//! context or the run mode fails these tests; so does an implementation that
//! accepts a malformed value, because each refusal names the flag that broke.
//!
//! Nothing here is a rendered screenshot: the run reports what a capture
//! *would* pin, it produces no image, and nothing is original-verified.

use std::path::PathBuf;
use std::process::Command;

use cs_app::camera::{CaptureOverride, ViewRig};
use cs_app::cli::{self, CliRequest};
use cs_app::run;
use cs_content::cameras::AspectRatio;
use cs_types::space::Quaternion;
use cs_types::{SceneProvenance, Tick};

fn args(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|arg| (*arg).to_string()).collect()
}

/// The flag vector of a complete capture request.
fn complete() -> Vec<String> {
    args(&[
        "--synthetic",
        "--headless",
        "--mission",
        "m01",
        "--tick",
        "600",
        "--pose",
        "100.5,-2.25,30",
        "--aspect",
        "16:9",
        "--view",
        "spyglass",
        "--settings",
        "fixed",
        "--trace",
        "private/capture.jsonl",
        "--seed",
        "7",
    ])
}

/// A complete request builds the typed [`cs_app::camera::CaptureRequest`]:
/// the mission, the tick and the pinned pose it requires, every optional
/// override when it is given, and the `seed`/`trace` run parameters the
/// synthetic run owns. The parse is pure: it needs no `CS_GAME_DIR` and no
/// installation, which is also what `--help` relies on.
#[test]
fn accept_f21_e_a_complete_request_builds_the_typed_capture() {
    match cli::parse(complete()) {
        CliRequest::Capture(request) => {
            let capture = &request.capture;
            assert_eq!(
                capture.mission().as_str(),
                "mission/m01",
                "a bare key becomes the canonical mission id"
            );
            assert_eq!(capture.tick(), Tick(600), "the capture's tick");
            let pose = capture.pose().expect("--pose pins a pose");
            assert_eq!(
                pose.position().to_array(),
                [100.5, -2.25, 30.0],
                "the pinned position is the flag's x,y,z"
            );
            assert_eq!(
                pose.rotation(),
                Quaternion::IDENTITY,
                "the flags carry no orientation, so the pinned rotation is identity"
            );
            assert_eq!(
                capture.aspect().map(AspectRatio::value),
                Some(16.0 / 9.0),
                "--aspect 16:9 is width over height"
            );
            assert_eq!(
                capture.rig(),
                Some(ViewRig::Spyglass),
                "--view spyglass selects the spyglass rig"
            );
            assert!(
                capture
                    .settings()
                    .expect("--settings fixed pins the fixed set")
                    .is_fixed(),
                "the pinned settings are the fixed comparison set"
            );
            assert_eq!(
                request.trace,
                Some(PathBuf::from("private/capture.jsonl")),
                "--trace is the run's own flag"
            );
            assert_eq!(request.seed, Some(7), "--seed is the run's own flag");
        }
        other => panic!("expected a capture request, got {other:?}"),
    }

    // The canonical namespaced id is accepted and names the same mission.
    match cli::parse(args(&[
        "--synthetic",
        "--headless",
        "--mission",
        "mission/m01",
        "--tick",
        "1",
        "--pose",
        "0,0,0",
    ])) {
        CliRequest::Capture(request) => assert_eq!(
            request.capture.mission().as_str(),
            "mission/m01",
            "the canonical form is the same id"
        ),
        other => panic!("expected a capture request, got {other:?}"),
    }

    // The optional flags stay optional: unset, the request pins nothing for
    // them and the report falls back to the frame's own values.
    match cli::parse(args(&[
        "--synthetic",
        "--headless",
        "--mission",
        "m01",
        "--tick",
        "1",
        "--pose",
        "0,0,0",
    ])) {
        CliRequest::Capture(request) => {
            assert_eq!(request.capture.rig(), None, "no --view, no rig pinned");
            assert_eq!(
                request.capture.aspect(),
                None,
                "no --aspect, the session's own aspect stands"
            );
            assert_eq!(
                request.capture.settings(),
                None,
                "no --settings, nothing pinned"
            );
            assert_eq!(request.seed, None);
            assert_eq!(request.trace, None);
        }
        other => panic!("expected a capture request, got {other:?}"),
    }
}

/// A `--mission` that names no mission is refused by name: a capture of an
/// airframe, a world or a malformed key is not a capture of a mission.
#[test]
fn accept_f21_e_a_non_mission_id_is_refused_naming_mission() {
    for value in [
        "airframe/bloodhawk",
        "world/c1c",
        "camera_track/intro",
        "bad!key",
        "",
        "/m01",
    ] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            value,
            "--tick",
            "1",
            "--pose",
            "0,0,0",
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--mission"),
                "{value:?}: the reason must name --mission, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
}

/// A non-numeric or negative tick is refused by name, mirroring `--ticks`.
#[test]
fn accept_f21_e_a_non_numeric_tick_is_refused_naming_tick() {
    for value in ["abc", "-5", "1.5", ""] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            value,
            "--pose",
            "0,0,0",
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--tick"),
                "{value:?}: the reason must name --tick, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
}

/// A pose that is not three finite components is refused by name: wrong
/// arity, a non-numeric part and a non-finite part are all unusable input.
#[test]
fn accept_f21_e_a_bad_pose_is_refused_naming_pose() {
    for value in [
        // wrong arity
        "1,2", "1,2,3,4", "1", "", "1,,3", // non-numeric
        "a,b,c", "1, 2, 3", // non-finite
        "nan,0,0", "1,inf,3", "1,2,-inf",
    ] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            "1",
            "--pose",
            value,
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--pose"),
                "{value:?}: the reason must name --pose, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
}

/// An unpinned or unknown `--settings` is refused by name: only `fixed`, the
/// one deterministic comparison set, can reproduce a frame.
#[test]
fn accept_f21_e_unpinned_settings_are_refused_naming_settings() {
    for value in ["unpinned", "loose", "default", "presentation", "FIXED", ""] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            "1",
            "--pose",
            "0,0,0",
            "--settings",
            value,
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--settings"),
                "{value:?}: the reason must name --settings, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
}

/// Every required flag is required: the mission the capture is of, the tick
/// it is reproducible at and the world pose it pins.
#[test]
fn accept_f21_e_a_missing_required_flag_is_refused() {
    let cases: [(&[&str], &str); 5] = [
        // no --mission
        (
            &[
                "--synthetic",
                "--headless",
                "--tick",
                "1",
                "--pose",
                "0,0,0",
            ],
            "--mission",
        ),
        // no --tick
        (
            &[
                "--synthetic",
                "--headless",
                "--mission",
                "m01",
                "--pose",
                "0,0,0",
            ],
            "--tick",
        ),
        // no --pose
        (
            &[
                "--synthetic",
                "--headless",
                "--mission",
                "m01",
                "--tick",
                "1",
            ],
            "--pose",
        ),
        // the capture's own flags without --synthetic
        (
            &["--mission", "m01", "--tick", "1", "--pose", "0,0,0"],
            "--tick",
        ),
        // the synthetic capture without --headless
        (
            &[
                "--synthetic",
                "--mission",
                "m01",
                "--tick",
                "1",
                "--pose",
                "0,0,0",
            ],
            "--headless",
        ),
    ];
    for (index, (raw, needle)) in cases.into_iter().enumerate() {
        match cli::parse(raw.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>()) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains(needle),
                "case {index}: the reason must name {needle}, got: {reason}"
            ),
            other => panic!("case {index} must be refused, got {other:?}"),
        }
    }
}

/// An aspect that is not a positive `w:h` ratio is refused by name, and a
/// `--view` outside the three declared rigs is refused by name.
#[test]
fn accept_f21_e_bad_aspect_and_view_are_refused() {
    for value in [
        "169", "16-9", "16:", ":9", "a:b", "0:0", "-4:3", "inf:9", "16:9:1", "",
    ] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            "1",
            "--pose",
            "0,0,0",
            "--aspect",
            value,
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--aspect"),
                "{value:?}: the reason must name --aspect, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
    for value in ["look", "external", "first-person", ""] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            "1",
            "--pose",
            "0,0,0",
            "--view",
            value,
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--view"),
                "{value:?}: the reason must name --view, got: {reason}"
            ),
            other => panic!("{value:?} must be refused, got {other:?}"),
        }
    }
}

/// The production run: `run_capture` steps the scene to the capture's tick
/// and returns the applied `CaptureReport`, whose overrides list everything
/// the request pinned — and the three it always states — plus the pinned
/// projection's narrowings.
#[test]
fn accept_f21_e_the_run_produces_the_applied_capture_report() {
    // The trace goes to a process-unique temp path: `private/` paths belong
    // to the operator, a test leaves no artefact behind, and a fixed name
    // would collide with another agent's run of this same binary on the
    // shared build host.
    let trace = std::env::temp_dir().join(format!(
        "cs_f21_e_capture_trace_{}.jsonl",
        std::process::id()
    ));
    let request = match cli::parse(args(&[
        "--synthetic",
        "--headless",
        "--mission",
        "m01",
        "--tick",
        "600",
        "--pose",
        "100.5,-2.25,30",
        "--aspect",
        "16:9",
        "--view",
        "spyglass",
        "--settings",
        "fixed",
        "--trace",
        trace.to_str().expect("the temp path is UTF-8"),
    ])) {
        CliRequest::Capture(request) => request,
        other => panic!("expected a capture request, got {other:?}"),
    };
    let report = run::run_capture(&request).expect("the capture run must succeed");
    let capture = &report.capture;

    assert_eq!(capture.mission().as_str(), "mission/m01");
    assert_eq!(capture.tick(), Tick(600), "the capture applies at its tick");
    assert_eq!(report.ticks_completed, 600, "the scene ran to the tick");
    assert_eq!(report.provenance, SceneProvenance::Synthetic);
    assert_eq!(report.final_sample.tick, Tick(600));
    assert_eq!(
        report.trace.as_deref(),
        Some(trace.as_path()),
        "the trace the run wrote is the one the flags named"
    );
    assert!(trace.exists(), "the trace file is really on disk");
    let _ = std::fs::remove_file(&trace);

    // Every override is reported, in the report's stable order: what the
    // request pinned first, then the three the engine always states.
    let fields: Vec<&str> = capture
        .overrides()
        .iter()
        .map(CaptureOverride::field)
        .collect();
    assert_eq!(
        fields,
        [
            "rig",
            "world_pose",
            "aspect",
            "settings",
            "smoothing",
            "magnification",
            "projection"
        ],
        "all overrides are reported, never dropped"
    );
    // The pinned pose held exactly, which also means the smoothing was
    // bypassed and the spyglass magnification folded into the frustum.
    assert_eq!(
        capture.pose().position().to_array(),
        [100.5, -2.25, 30.0],
        "the report names the pose the capture drew from"
    );
    assert_eq!(
        capture.aspect().value(),
        16.0 / 9.0,
        "the report names the aspect the capture framed at"
    );
    match capture.override_of("smoothing") {
        Some(CaptureOverride::Smoothing { bypassed }) => {
            assert!(*bypassed, "a pinned pose bypasses the smoother")
        }
        other => panic!("the smoothing override must be reported, got {other:?}"),
    }
    match capture.override_of("magnification") {
        Some(CaptureOverride::Magnification {
            factor,
            folded_into_fov,
        }) => {
            assert_eq!(*factor, 4.0, "the spyglass mode declares 4x");
            assert!(*folded_into_fov, "the factor is folded into the fov");
        }
        other => panic!("the magnification override must be reported, got {other:?}"),
    }
    // The f64 -> f32 narrowing is visible per pinned field, with both ends
    // kept: the declared policy's value and what the record carries.
    for narrowing in capture.pinned().narrowings() {
        assert!(
            narrowing.exact.is_finite() && narrowing.pinned.is_finite(),
            "{}: both ends of the narrowing are reported",
            narrowing.field
        );
    }
}

/// The real binary end to end: the flags run to the capture's tick and the
/// applied report prints every override plus the narrowing — never as an
/// image file, and never as success text that hides a failure.
#[test]
fn accept_f21_e_the_cs_binary_prints_the_applied_report() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .args([
            "--synthetic",
            "--headless",
            "--mission",
            "m01",
            "--tick",
            "4",
            "--pose",
            "0,10,0",
            "--aspect",
            "16:9",
            "--view",
            "spyglass",
            "--settings",
            "fixed",
        ])
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "the capture run must exit zero; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mission/m01") && stdout.contains("tick 4"),
        "the report names the mission and tick, got: {stdout:?}"
    );
    for needle in [
        "rig",
        "world_pose",
        "aspect",
        "settings",
        "smoothing",
        "magnification",
        "projection",
        "narrowed",
    ] {
        assert!(
            stdout.contains(needle),
            "the printed report must include {needle}, got: {stdout:?}"
        );
    }
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "a successful run stays silent on stderr, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The real binary reports invalid input as exit 2 with a diagnostic naming
/// the offending flag, and prints no report — an implementation that always
/// exits zero fails here.
#[test]
fn accept_f21_e_the_cs_binary_reports_bad_flags_by_name() {
    let cases: [(&[&str], &str); 3] = [
        (
            &[
                "--synthetic",
                "--headless",
                "--mission",
                "m01",
                "--tick",
                "x",
                "--pose",
                "0,0,0",
            ],
            "--tick",
        ),
        (
            &[
                "--synthetic",
                "--headless",
                "--mission",
                "airframe/bloodhawk",
                "--tick",
                "1",
                "--pose",
                "0,0,0",
            ],
            "--mission",
        ),
        (
            &[
                "--synthetic",
                "--headless",
                "--mission",
                "m01",
                "--tick",
                "1",
            ],
            "--pose",
        ),
    ];
    for (index, (raw, needle)) in cases.into_iter().enumerate() {
        let output = Command::new(env!("CARGO_BIN_EXE_cs"))
            .args(raw)
            .env_remove("CS_GAME_DIR")
            .output()
            .expect("the cs binary must run without a GPU or retail installation");
        assert_eq!(
            output.status.code(),
            Some(i32::from(cli::EXIT_INVALID_INPUT)),
            "case {index} must exit 2, got {:?}",
            output.status.code()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cs:") && stderr.contains(needle),
            "case {index}: the diagnostic must name the binary and {needle}, got: {stderr:?}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("override"),
            "case {index}: no capture may be reported, got: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
