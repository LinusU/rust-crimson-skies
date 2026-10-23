//! Task #1832 `PARITY5-START-FLOW.04`: the owner-command smoke for the whole
//! front-end sequence, `cs --cs-path <retail> --smoke-seconds <n>
//! --capture-dir <dir>`. Task test prefix: `accept_parity5_start_flow_04_`.
//!
//! The command-line half runs the production parser and the chain's own
//! argument builders ([`handover_args`], [`return_args_smoke`]): the bare
//! command, its four legs and their capture subdirectories all come out of
//! the same functions the running chain calls. The `#[ignore]`d retail test
//! drives the real binary end to end under `$CS_GAME_DIR`.

use std::path::PathBuf;

use cs_app::cli::{self, CliRequest};
use cs_app::playtest::SmokeRequest;
use cs_app::ui::front_end::{
    FIRST_MISSION, FRONT_END_SMOKE_SUBDIR, MISSION_SMOKE_SUBDIR, MissionEnd, REPLAY_SMOKE_SUBDIR,
    RESUMED_SMOKE_SUBDIR, StartFlowRequest, handover_args, return_args_smoke,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

/// The bare command is the start flow with a smoke: no `--mission`, no
/// `--front-end`, no shortcut.
#[test]
fn accept_parity5_start_flow_04_bare_command_is_the_start_flow_smoke() {
    let parse = cli::parse(args(&[
        "--cs-path",
        "/retail",
        "--smoke-seconds",
        "60",
        "--capture-dir",
        "/out",
    ]));
    match parse {
        CliRequest::StartFlow(request) => {
            assert_eq!(request.cs_path, PathBuf::from("/retail"));
            assert_eq!(request.resume, None);
            assert_eq!(
                request.smoke,
                Some(SmokeRequest {
                    seconds: 60,
                    capture_dir: PathBuf::from("/out"),
                })
            );
        }
        other => panic!("the bare owner-command smoke parses to {other:?}"),
    }
    // The default capture directory is the front-end's own.
    match cli::parse(args(&["--cs-path", "/retail", "--smoke-seconds", "60"])) {
        CliRequest::StartFlow(request) => {
            assert!(request.smoke.is_some_and(|smoke| smoke.seconds == 60));
        }
        other => panic!("{other:?}"),
    }
    // The mission smoke's floor applies: the mission legs are its own run.
    assert!(matches!(
        cli::parse(args(&["--cs-path", "/retail", "--smoke-seconds", "9"])),
        CliRequest::Invalid { .. }
    ));
    // `--capture-dir` with no `--smoke-seconds` is no run at all.
    assert!(matches!(
        cli::parse(args(&["--cs-path", "/retail", "--capture-dir", "/out"])),
        CliRequest::Invalid { .. }
    ));
    // `--smoke-start-flow` on the bare command names no request: the flag is
    // the chain's own, a mode of `--mission` legs only.
    let mut line = args(&["--cs-path", "/retail", "--smoke-seconds", "60"]);
    line.push("--smoke-start-flow".to_owned());
    assert!(
        matches!(cli::parse(line.clone()), CliRequest::Invalid { .. }),
        "{line:?} is not a runnable request"
    );
    // A hand-typed `--smoke-start-flow` mission needs the smoke too.
    assert!(matches!(
        cli::parse(args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-start-flow"
        ])),
        CliRequest::Invalid { .. }
    ));
}

/// The resumed leg keeps the smoke: `--resume-start-flow <end>` combines
/// with `--smoke-seconds`/`--capture-dir` and nothing else.
#[test]
fn accept_parity5_start_flow_04_resumed_leg_keeps_the_smoke() {
    match cli::parse(args(&[
        "--cs-path",
        "/retail",
        "--resume-start-flow",
        "abandoned",
        "--smoke-seconds",
        "60",
        "--capture-dir",
        "/out",
    ])) {
        CliRequest::StartFlow(request) => {
            assert_eq!(request.resume, Some(MissionEnd::Abandoned));
            assert_eq!(
                request.smoke,
                Some(SmokeRequest {
                    seconds: 60,
                    capture_dir: PathBuf::from("/out"),
                })
            );
        }
        other => panic!("the resumed leg parses to {other:?}"),
    }
    // The other smoke-mode flags still refuse the resume, and `--mission`
    // with a value names a leg the resumed front end is not.
    for extra in [
        &["--smoke-no-teleport"][..],
        &["--smoke-engagement"][..],
        &["--smoke-failure"][..],
        &["--smoke-start-flow"][..],
        &["--mission", "M01"][..],
    ] {
        let mut line = args(&["--cs-path", "/retail", "--resume-start-flow", "cabin"]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        assert!(
            matches!(cli::parse(line.clone()), CliRequest::Invalid { .. }),
            "{line:?} is not a runnable request"
        );
    }
}

/// The chain the owner-command smoke runs, built and re-parsed through the
/// production argument builders: four legs, each named by its capture
/// directory, bounded — the replay never returns to the front end.
#[test]
fn accept_parity5_start_flow_04_chain_args_round_trip_and_terminate() {
    let smoke = SmokeRequest {
        seconds: 60,
        capture_dir: PathBuf::from("/out"),
    };
    // Leg 1: the front end hands M01 over — `--from-start-flow` so its Quit
    // comes back, and its own smoke flags on `mission/`.
    let first = StartFlowRequest {
        cs_path: PathBuf::from("/retail"),
        resume: None,
        smoke: Some(smoke.clone()),
    };
    let mission = match cli::parse(handover_args(&first, FIRST_MISSION)) {
        CliRequest::Mission(mission) => mission,
        other => panic!("the first hand-over parses to {other:?}"),
    };
    assert!(mission.from_start_flow);
    assert!(mission.smoke_start_flow);
    assert_eq!(
        mission.smoke,
        Some(SmokeRequest {
            seconds: 60,
            capture_dir: PathBuf::from("/out").join(MISSION_SMOKE_SUBDIR),
        })
    );

    // Leg 2: the mission's Quit resumes the front end on `resumed/`, still
    // a smoke.
    let resumed = match cli::parse(return_args_smoke(
        &PathBuf::from("/retail"),
        MissionEnd::Abandoned,
        mission.smoke.as_ref(),
    )) {
        CliRequest::StartFlow(request) => request,
        other => panic!("the return parses to {other:?}"),
    };
    assert_eq!(resumed.resume, Some(MissionEnd::Abandoned));
    let resumed_smoke = resumed
        .smoke
        .clone()
        .expect("the resumed leg keeps the smoke");
    assert_eq!(resumed_smoke.seconds, 60);
    // The mission leg's own directory was `mission/`; the root comes back so
    // the resumed leg's `resumed/` lands beside it.
    assert_eq!(resumed_smoke.capture_dir, PathBuf::from("/out"));

    // Leg 3: the cabin's Next Mission replays M01 — still a smoke on
    // `replay/`, but never `--from-start-flow`: the chain ends with the
    // process, it cannot loop.
    let replay = match cli::parse(handover_args(&resumed, FIRST_MISSION)) {
        CliRequest::Mission(mission) => mission,
        other => panic!("the replay hand-over parses to {other:?}"),
    };
    assert!(!replay.from_start_flow, "the replay ends the chain");
    assert!(replay.smoke_start_flow);
    assert_eq!(
        replay.smoke,
        Some(SmokeRequest {
            seconds: 60,
            capture_dir: resumed_smoke.capture_dir.join(REPLAY_SMOKE_SUBDIR),
        })
    );

    // An interactive hand-over is unchanged: `--from-start-flow` only.
    let interactive = StartFlowRequest {
        cs_path: PathBuf::from("/retail"),
        resume: None,
        smoke: None,
    };
    match cli::parse(handover_args(&interactive, FIRST_MISSION)) {
        CliRequest::Mission(mission) => {
            assert!(mission.from_start_flow);
            assert!(!mission.smoke_start_flow);
            assert!(mission.smoke.is_none());
        }
        other => panic!("{other:?}"),
    }
    // The subdirectory constants name every leg's directory.
    for subdir in [
        FRONT_END_SMOKE_SUBDIR,
        MISSION_SMOKE_SUBDIR,
        RESUMED_SMOKE_SUBDIR,
        REPLAY_SMOKE_SUBDIR,
    ] {
        assert!(!subdir.is_empty() && !subdir.contains('/'));
    }
}

/// The end-to-end run: the real `cs` binary drives the whole chain under
/// `$CS_GAME_DIR`. Four process images, four capture directories, every
/// report's checks green. Ignored without the retail installation and a
/// window.
#[test]
#[ignore = "requires CS_GAME_DIR, a GPU and the built cs binary"]
fn accept_parity5_start_flow_04_owner_command_smoke_runs_the_whole_flow() {
    let cs_path = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR must name the original installation for this retail test");
    let capture = std::env::temp_dir().join("parity5-start-flow-04-smoke");
    let _ = std::fs::remove_dir_all(&capture);
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args([
            "--cs-path",
            &cs_path,
            "--smoke-seconds",
            "60",
            "--capture-dir",
            &capture.display().to_string(),
        ])
        .status()
        .expect("the cs binary spawns");
    assert!(
        status.success(),
        "the start-flow smoke chain failed: {status}"
    );
    for leg in ["front-end", "mission", "resumed", "replay"] {
        let report = capture.join(leg).join("report.json");
        assert!(
            report.exists(),
            "the {leg} leg wrote no report at {report:?}"
        );
        let body = std::fs::read_to_string(&report).expect("the report reads");
        assert!(
            body.contains("\"passed\":true"),
            "the {leg} leg's checks failed: {body}"
        );
    }
}
