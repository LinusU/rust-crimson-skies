//! Command-line request parsing for the `cs` binary.
//!
//! Parsing is a pure function of the argument vector: it never reads
//! `CS_GAME_DIR`, opens the retail installation, starts asset discovery or
//! touches the file system. That is what makes `--help` and `--version` work
//! without a GPU and without a retail installation (F00 non-negotiable
//! behavior 3, acceptance case AC02 in
//! `specs/F00-workspace-toolchain-and-first-executable.md`).
//!
//! F00-C adds the one run mode this workspace can honestly serve: the
//! fixed-tick headless synthetic smoke named in
//! `docs/contracts/CLI-EVIDENCE.md` (`--synthetic --headless --ticks <n>`
//! with an optional `--trace <file>`). A successful parse produces the typed
//! [`SyntheticRequest`] that [`crate::run::run_synthetic`] consumes; every
//! other argument vector stays a failure reported on stderr with exit code
//! [`EXIT_INVALID_INPUT`], never as success.

use std::path::PathBuf;

use crate::mission_session::{DEFAULT_MISSION_CAPTURE_DIR, MIN_MISSION_SMOKE_SECONDS};
use crate::playtest::smoke::MIN_SMOKE_SECONDS;
use crate::playtest::{DEFAULT_CAPTURE_DIR, PlaytestRequest, RetailRequest, SmokeRequest};
use crate::run::SyntheticRequest;
use crate::ui::front_end::{
    DEFAULT_FRONT_END_CAPTURE_DIR, FrontEndRequest, MIN_SHELL_SMOKE_SECONDS, MissionEnd,
    StartFlowRequest,
};

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
pub const EXIT_INVALID_INPUT: u8 = 2;

/// Exit code for a runtime failure: anything that is not invalid input (2),
/// failed validation (3) or a missing capability (4) per
/// `docs/contracts/CLI-EVIDENCE.md`. A failure is never reported as zero.
pub const EXIT_RUNTIME_FAILURE: u8 = 1;

/// A `--cs-path <dir> --mission <id>` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionRequest {
    /// The read-only original installation.
    pub cs_path: PathBuf,
    /// The work-order label of the mission, e.g. `M01`.
    pub mission: String,
    /// The scripted owner-command smoke (`--smoke-seconds <n>
    /// [--capture-dir <dir>]`), when one was asked for. The run then drives
    /// the same windowed composition and fails when a check fails (Rally
    /// #1535 `M01-OWNER-SMOKE`).
    pub smoke: Option<SmokeRequest>,
    /// `--smoke-no-teleport`: the smoke flies the measured start for the whole
    /// run, never moves the player and judges only whether the wingmen fly
    /// near it (Rally #1572 `M01-SMOKE-NO-TELEPORT`).
    pub smoke_no_teleport: bool,
    /// `--dev-overlay`: draw the developer overlay (telemetry, key help,
    /// contact list, designed-layout captions) on top of the cockpit HUD
    /// (Rally #1641 `PARITY-M01-HUD`).
    pub dev_overlay: bool,
    /// `--dev-keys`: fly with the developer key layout instead of the
    /// original game's default controls (Rally #1733
    /// `PARITY2-ORIGINAL-KEYBINDINGS`).
    pub dev_keys: bool,
    /// `--smoke-engagement`: the smoke flies the measured start with no input
    /// for the whole `--smoke-seconds` budget and judges whether the mission's
    /// hostile aircraft damage the player (Rally #1653
    /// `PARITY-M01-OUTCOME-A`).
    pub smoke_engagement: bool,
    /// `--smoke-failure`: the smoke flies the measured start, kills the
    /// player through the session's damage authority once control is theirs
    /// and judges the journal's debrief screen the failed run opens (Rally
    /// #1655 `PARITY-M01-OUTCOME-C`).
    pub smoke_failure: bool,
    /// `--smoke-crash`: the smoke dives the player onto the terrain, lets the
    /// production terrain-contact rule crash it and photographs the crash
    /// flow: contact effects, the detached camera with `Fatal Crash!` and the
    /// Mission Failed screen after a key (Rally #1857 `PARITY4-CRASH-E`).
    pub smoke_crash: bool,
    /// `--smoke-trucks`: the smoke walks the declared objective route until the
    /// mission script's own `WAKEUP_TURRETS` wakes the convoy trucks, places
    /// the player in their range and judges the turret fire, the hull drop and
    /// framebuffer screenshots of the trucks (Rally #1926).
    pub smoke_trucks: bool,
    /// `--smoke-convoy-kill`: the smoke places the player in front of the
    /// bridge convoy, presses the guns key until the damage authority has
    /// destroyed every truck and judges framebuffer screenshots of the intact
    /// and the destroyed convoy (Rally #2047).
    pub smoke_convoy_kill: bool,
    /// `--smoke-success`: the smoke places the player inside `OBJECTIVE12`'s
    /// radius of the `shipwreck` site and records which objective blocks the
    /// host reported completed (Rally #2081 `M01-SMOKE-SUCCESS-A`).
    pub smoke_success: bool,
    /// `--from-start-flow`: the mission was handed over by the start flow
    /// (`cs --cs-path <dir>`), so leaving it for the cabin (the debrief's
    /// Return to Cabin, the pause map's Quit) re-opens the front end instead
    /// of ending the process (Rally #1831 `PARITY5-START-FLOW.03`).
    pub from_start_flow: bool,
    /// The host joystick and its force feedback (`--no-joystick`,
    /// `--joystick <device>`, `--force-feedback-gain <percent>`,
    /// `--no-force-feedback`; SIDEWINDER-FFB2, #1968).
    pub joystick: crate::input::hid::HostJoystickOptions,
    /// `--smoke-start-flow`: the mission is one leg of the owner-command
    /// start-flow smoke (`cs --cs-path <dir> --smoke-seconds <n>`, Rally #1832
    /// `PARITY5-START-FLOW.04`): the smoke goes through the briefing, the
    /// Flight Check and the launch cinematic like the scripted mode, then
    /// flies a short flight, opens the Esc pause map and presses **Quit** —
    /// the outcome the chain's next leg resumes (or ends) on.
    pub smoke_start_flow: bool,
}

/// A `--joystick-probe` request: list the host's joysticks, show one's controls
/// live and, when asked, play a low-strength force-feedback test sequence
/// (SIDEWINDER-FFB2, #1968).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoystickProbeRequest {
    /// Which joystick (`--joystick <vid:pid|name>`); the first otherwise.
    pub filter: Option<String>,
    /// Seconds of live input to show (`--probe-seconds <n>`).
    pub seconds: u32,
    /// `--test-force-feedback`: play the test sequence.
    pub test_force_feedback: bool,
    /// The test sequence's strength (`--force-feedback-gain <percent>`).
    pub gain_percent: u32,
    /// `--save-descriptor <file>`: write the raw HID report descriptor there.
    pub save_descriptor: Option<PathBuf>,
}

/// What the caller asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliRequest {
    /// `-h`/`--help`: print [`HELP_TEXT`] on stdout and exit 0.
    Help,
    /// `-V`/`--version`: print [`version_text`] on stdout and exit 0.
    Version,
    /// `--synthetic --headless --ticks <n> [--trace <file>] [--seed <u64>]`:
    /// run the asset-free `SYNTHETIC` scene for exactly that many fixed
    /// ticks, optionally recording a trace and optionally starting from a
    /// root seed.
    Synthetic(SyntheticRequest),
    /// `--playtest [--cs-path <dir> [--world <id>] [--aircraft <id>]]
    /// [--smoke-seconds <n> [--capture-dir <dir>]]`: open the windowed
    /// development playtest (task #647) over the synthetic scene or, with
    /// `--cs-path`, over original assets (task #649), or run its finite
    /// deterministic smoke.
    Playtest(PlaytestRequest),
    /// `--cs-path <dir> --mission <id>`: launch one original mission through
    /// [`crate::mission_launch::launch_mission`] (task #359). A mission whose
    /// launch closure is not satisfied exits nonzero with its source
    /// diagnostics.
    Mission(MissionRequest),
    /// `--cs-path <dir> --front-end [--smoke-seconds <n> [--capture-dir
    /// <dir>]]`: open the windowed front end presenting the original
    /// `ASSETS/LAYOUT.CSV` sections (FE-MENU-1, task #1833).
    FrontEnd(FrontEndRequest),
    /// `--cs-path <dir>` alone: open the windowed start flow on the original
    /// main menu, driven by the F45 front-end machine (Rally #1830).
    StartFlow(StartFlowRequest),
    /// `--joystick-probe [--joystick <device>] [--probe-seconds <n>]
    /// [--test-force-feedback [--force-feedback-gain <percent>]]
    /// [--save-descriptor <file>]`: check a host joystick
    /// ([`crate::input::hid::probe`]).
    JoystickProbe(JoystickProbeRequest),
    /// No arguments at all: invalid input, reported on stderr and exit 2.
    MissingInput,
    /// Arguments that name no supported run mode: reported on stderr and
    /// exit 2.
    Unsupported { args: Vec<String> },
    /// A recognized flag was given an unusable value or a combination that
    /// names no runnable request: reported on stderr with a specific reason
    /// and exit 2.
    Invalid { reason: String },
}

/// Classifies the argument vector left after the program name.
///
/// `--help`/`--version` win in the order they appear, so they work whatever
/// else was typed. Everything else must form a complete
/// `--synthetic --headless --ticks <n> [--trace <file>] [--seed <u64>]`
/// request; a vector that is unknown or structurally incomplete becomes
/// [`CliRequest::Unsupported`], and one that is complete but unusable (a
/// non-numeric tick count, a non-`u64` seed, `--synthetic` without
/// `--headless`, `--seed` without `--synthetic`) becomes
/// [`CliRequest::Invalid`] with a reason naming the offending flag.
///
/// Parsing never touches the environment or the file system.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> CliRequest {
    let args: Vec<String> = args.into_iter().collect();
    if args.is_empty() {
        return CliRequest::MissingInput;
    }
    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => return CliRequest::Help,
            "--version" | "-V" => return CliRequest::Version,
            _ => {}
        }
    }

    let mut synthetic = false;
    let mut headless = false;
    let mut ticks: Option<u64> = None;
    let mut trace: Option<PathBuf> = None;
    let mut seed: Option<u64> = None;
    let mut playtest = false;
    let mut smoke_seconds: Option<u32> = None;
    let mut capture_dir: Option<PathBuf> = None;
    let mut cs_path: Option<PathBuf> = None;
    let mut world: Option<String> = None;
    let mut aircraft: Option<String> = None;
    let mut mission: Option<String> = None;
    let mut smoke_no_teleport = false;
    let mut dev_overlay = false;
    let mut dev_keys = false;
    let mut smoke_engagement = false;
    let mut smoke_failure = false;
    let mut smoke_crash = false;
    let mut smoke_trucks = false;
    let mut smoke_convoy_kill = false;
    let mut smoke_success = false;
    let mut smoke_start_flow = false;
    let mut front_end = false;
    let mut from_start_flow = false;
    let mut resume_start_flow: Option<String> = None;
    let mut joystick_probe = false;
    let mut no_joystick = false;
    let mut joystick_filter: Option<String> = None;
    let mut force_feedback_gain: Option<u32> = None;
    let mut no_force_feedback = false;
    let mut test_force_feedback = false;
    let mut probe_seconds: Option<u32> = None;
    let mut save_descriptor: Option<PathBuf> = None;
    let mut unknown = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].clone();
        let mut values = 0;
        match arg.as_str() {
            "--synthetic" => synthetic = true,
            "--headless" => headless = true,
            "--playtest" => playtest = true,
            "--smoke-no-teleport" => smoke_no_teleport = true,
            "--dev-overlay" => dev_overlay = true,
            "--dev-keys" => dev_keys = true,
            "--smoke-engagement" => smoke_engagement = true,
            "--smoke-failure" => smoke_failure = true,
            "--smoke-crash" => smoke_crash = true,
            "--smoke-trucks" => smoke_trucks = true,
            "--smoke-convoy-kill" => smoke_convoy_kill = true,
            "--smoke-success" => smoke_success = true,
            "--smoke-start-flow" => smoke_start_flow = true,
            "--front-end" => front_end = true,
            "--from-start-flow" => from_start_flow = true,
            "--joystick-probe" => joystick_probe = true,
            "--no-joystick" => no_joystick = true,
            "--no-force-feedback" => no_force_feedback = true,
            "--test-force-feedback" => test_force_feedback = true,
            "--joystick" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                joystick_filter = Some(value);
                values = 1;
            }
            "--save-descriptor" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                save_descriptor = Some(PathBuf::from(value));
                values = 1;
            }
            "--force-feedback-gain" | "--probe-seconds" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                let Ok(parsed) = value.parse::<u32>() else {
                    return CliRequest::Invalid {
                        reason: format!("{arg} expects a whole number, found {value:?}"),
                    };
                };
                if arg == "--force-feedback-gain" {
                    if let Err(error) =
                        crate::force_feedback::ForceFeedbackSettings::with_gain(parsed)
                    {
                        return CliRequest::Invalid {
                            reason: format!("--force-feedback-gain: {error}"),
                        };
                    }
                    force_feedback_gain = Some(parsed);
                } else {
                    probe_seconds = Some(parsed);
                }
                values = 1;
            }
            "--resume-start-flow" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                resume_start_flow = Some(value);
                values = 1;
            }
            "--smoke-seconds" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                match value.parse::<u32>() {
                    Ok(parsed) => smoke_seconds = Some(parsed),
                    Err(_) => {
                        return CliRequest::Invalid {
                            reason: format!(
                                "--smoke-seconds expects a whole number of seconds, found {value:?}"
                            ),
                        };
                    }
                }
                values = 1;
            }
            "--cs-path" | "--world" | "--aircraft" | "--mission" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                match arg.as_str() {
                    "--cs-path" => cs_path = Some(PathBuf::from(value)),
                    "--world" => world = Some(value),
                    "--mission" => mission = Some(value),
                    _ => aircraft = Some(value),
                }
                values = 1;
            }
            "--capture-dir" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                capture_dir = Some(PathBuf::from(value));
                values = 1;
            }
            "--ticks" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    // The flag is the last thing typed: no request can be
                    // formed from this vector at all.
                    return CliRequest::Unsupported { args };
                };
                match value.parse::<u64>() {
                    Ok(parsed) => ticks = Some(parsed),
                    Err(_) => {
                        return CliRequest::Invalid {
                            reason: format!(
                                "--ticks expects a non-negative integer tick count, found {value:?}"
                            ),
                        };
                    }
                }
                values = 1;
            }
            "--seed" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    // Mirror `--ticks`: a flag whose value is missing names no
                    // request at all, so the vector stays unsupported input.
                    return CliRequest::Unsupported { args };
                };
                match value.parse::<u64>() {
                    // A duplicate `--seed` keeps the later value, like every
                    // other value-carrying flag here.
                    Ok(parsed) => seed = Some(parsed),
                    Err(_) => {
                        return CliRequest::Invalid {
                            reason: format!("--seed expects a u64 root seed, found {value:?}"),
                        };
                    }
                }
                values = 1;
            }
            "--trace" => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return CliRequest::Unsupported { args };
                };
                trace = Some(PathBuf::from(value));
                values = 1;
            }
            _ => unknown = true,
        }
        index += 1 + values;
    }

    if unknown {
        return CliRequest::Unsupported { args };
    }
    if joystick_probe {
        if mission.is_some()
            || cs_path.is_some()
            || playtest
            || synthetic
            || front_end
            || no_joystick
            || no_force_feedback
            || smoke_seconds.is_some()
        {
            return CliRequest::Invalid {
                reason: "--joystick-probe checks a host joystick on its own; it takes only \
 --joystick, --probe-seconds, --test-force-feedback, --force-feedback-gain and \
 --save-descriptor"
                    .to_string(),
            };
        }
        return CliRequest::JoystickProbe(JoystickProbeRequest {
            filter: joystick_filter,
            seconds: probe_seconds.unwrap_or(10),
            test_force_feedback,
            gain_percent: force_feedback_gain
                .unwrap_or(crate::force_feedback::DESIGNED_DEFAULT_GAIN_PERCENT),
            save_descriptor,
        });
    }
    if test_force_feedback || probe_seconds.is_some() || save_descriptor.is_some() {
        return CliRequest::Invalid {
            reason: "--test-force-feedback, --probe-seconds and --save-descriptor are flags of \
 --joystick-probe"
                .to_string(),
        };
    }
    let joystick_flags = no_joystick
        || joystick_filter.is_some()
        || force_feedback_gain.is_some()
        || no_force_feedback;
    if joystick_flags && mission.is_none() {
        return CliRequest::Invalid {
            reason: "--no-joystick, --joystick, --force-feedback-gain and --no-force-feedback \
 are flags of --cs-path <dir> --mission <id> (or --joystick-probe)"
                .to_string(),
        };
    }
    if no_joystick && (joystick_filter.is_some() || force_feedback_gain.is_some()) {
        return CliRequest::Invalid {
            reason: "--no-joystick does not combine with --joystick or --force-feedback-gain"
                .to_string(),
        };
    }
    if no_force_feedback && force_feedback_gain.is_some() {
        return CliRequest::Invalid {
            reason: "--no-force-feedback does not combine with --force-feedback-gain".to_string(),
        };
    }
    let joystick = crate::input::hid::HostJoystickOptions {
        enabled: !no_joystick,
        filter: joystick_filter,
        force_feedback: if no_force_feedback || no_joystick {
            crate::force_feedback::ForceFeedbackSettings::off()
        } else {
            crate::force_feedback::ForceFeedbackSettings::with_gain(
                force_feedback_gain.unwrap_or(crate::force_feedback::DESIGNED_DEFAULT_GAIN_PERCENT),
            )
            .expect("the gain was checked where it was parsed")
        },
    };
    if dev_overlay && mission.is_none() {
        return CliRequest::Invalid {
            reason: "--dev-overlay is a flag of --cs-path <dir> --mission <id>".to_string(),
        };
    }
    if dev_keys && mission.is_none() {
        return CliRequest::Invalid {
            reason: "--dev-keys is a flag of --cs-path <dir> --mission <id>".to_string(),
        };
    }
    if smoke_no_teleport && (mission.is_none() || smoke_seconds.is_none()) {
        return CliRequest::Invalid {
            reason: "--smoke-no-teleport is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n>"
                .to_string(),
        };
    }
    if smoke_engagement && (mission.is_none() || smoke_seconds.is_none() || smoke_no_teleport) {
        return CliRequest::Invalid {
            reason: "--smoke-engagement is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport"
                .to_string(),
        };
    }
    if smoke_failure
        && (mission.is_none() || smoke_seconds.is_none() || smoke_no_teleport || smoke_engagement)
    {
        return CliRequest::Invalid {
            reason: "--smoke-failure is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport or --smoke-engagement"
                .to_string(),
        };
    }
    if smoke_crash
        && (mission.is_none()
            || smoke_seconds.is_none()
            || smoke_no_teleport
            || smoke_engagement
            || smoke_failure)
    {
        return CliRequest::Invalid {
            reason: "--smoke-crash is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport, --smoke-engagement or \
 --smoke-failure"
                .to_string(),
        };
    }
    if smoke_trucks
        && (mission.is_none()
            || smoke_seconds.is_none()
            || smoke_no_teleport
            || smoke_engagement
            || smoke_failure
            || smoke_crash
            || smoke_convoy_kill
            || smoke_success)
    {
        return CliRequest::Invalid {
            reason: "--smoke-trucks is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport, --smoke-engagement, \
 --smoke-failure or --smoke-crash"
                .to_string(),
        };
    }
    if smoke_convoy_kill
        && (mission.is_none()
            || smoke_seconds.is_none()
            || smoke_no_teleport
            || smoke_engagement
            || smoke_failure
            || smoke_crash
            || smoke_success)
    {
        return CliRequest::Invalid {
            reason: "--smoke-convoy-kill is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport, --smoke-engagement, \
 --smoke-failure, --smoke-crash or --smoke-trucks"
                .to_string(),
        };
    }
    if smoke_success
        && (mission.is_none()
            || smoke_seconds.is_none()
            || smoke_no_teleport
            || smoke_engagement
            || smoke_failure
            || smoke_crash
            || smoke_trucks
            || smoke_convoy_kill)
    {
        return CliRequest::Invalid {
            reason: "--smoke-success is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport, --smoke-engagement, \
 --smoke-failure, --smoke-crash, --smoke-trucks or --smoke-convoy-kill"
                .to_string(),
        };
    }
    if smoke_start_flow
        && (mission.is_none()
            || smoke_seconds.is_none()
            || smoke_no_teleport
            || smoke_engagement
            || smoke_failure
            || smoke_crash
            || smoke_trucks
            || smoke_convoy_kill
            || smoke_success)
    {
        return CliRequest::Invalid {
            reason: "--smoke-start-flow is a mode of --cs-path <dir> --mission <id> \
 --smoke-seconds <n> and does not combine with --smoke-no-teleport, --smoke-engagement or \
 --smoke-failure or --smoke-crash"
                .to_string(),
        };
    }
    if from_start_flow && mission.is_none() {
        return CliRequest::Invalid {
            reason: "--from-start-flow is a mode of --cs-path <dir> --mission <id>: it is how \
 the start flow hands a mission over"
                .to_string(),
        };
    }
    if resume_start_flow.is_some()
        && (mission.is_some()
            || front_end
            || playtest
            || synthetic
            || headless
            || world.is_some()
            || aircraft.is_some()
            || ticks.is_some()
            || trace.is_some()
            || seed.is_some()
            || smoke_no_teleport
            || dev_overlay
            || dev_keys
            || smoke_engagement
            || smoke_failure
            || smoke_crash
            || smoke_trucks
            || smoke_convoy_kill
            || smoke_success
            || smoke_start_flow)
    {
        return CliRequest::Invalid {
            reason: "--resume-start-flow re-opens the start flow (`cs --cs-path <dir>`) after a \
 mission; it combines with no flag but the owner-command smoke's --smoke-seconds <n> \
 [--capture-dir <dir>] (#1832)"
                .to_string(),
        };
    }
    if front_end {
        if playtest
            || synthetic
            || headless
            || mission.is_some()
            || world.is_some()
            || aircraft.is_some()
            || ticks.is_some()
            || trace.is_some()
            || seed.is_some()
        {
            return CliRequest::Invalid {
                reason: "--front-end is the windowed original front end; it does not \
 combine with --playtest, --synthetic, --headless, --mission, --world, --aircraft, \
 --ticks, --trace or --seed"
                    .to_string(),
            };
        }
        let Some(cs_path) = cs_path else {
            return CliRequest::Invalid {
                reason: "--front-end needs --cs-path <dir>: it presents the original \
 installation's own LAYOUT.CSV sections, and there is no synthetic stand-in"
                    .to_string(),
            };
        };
        if capture_dir.is_some() && smoke_seconds.is_none() {
            return CliRequest::Invalid {
                reason: "--capture-dir is where --front-end --smoke-seconds <n> writes \
 its artifacts; the interactive front end writes none"
                    .to_string(),
            };
        }
        let smoke = match smoke_seconds {
            Some(seconds) if seconds < MIN_SHELL_SMOKE_SECONDS => {
                return CliRequest::Invalid {
                    reason: format!(
                        "--smoke-seconds must be at least {MIN_SHELL_SMOKE_SECONDS}: \
 the scripted front-end pass needs that long to hover, press, navigate, return and quit"
                    ),
                };
            }
            Some(seconds) => Some(SmokeRequest {
                seconds,
                capture_dir: capture_dir
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_FRONT_END_CAPTURE_DIR)),
            }),
            None => None,
        };
        return CliRequest::FrontEnd(FrontEndRequest { cs_path, smoke });
    }
    if let Some(mission) = mission {
        if playtest || synthetic || headless || world.is_some() || aircraft.is_some() {
            return CliRequest::Invalid {
                reason: "--mission launches one original mission and does not combine with \
 --playtest, --synthetic, --headless, --world or --aircraft"
                    .to_string(),
            };
        }
        let Some(cs_path) = cs_path else {
            return CliRequest::Invalid {
                reason: "--mission needs --cs-path <dir>: an original mission has no \
 synthetic stand-in"
                    .to_string(),
            };
        };
        if capture_dir.is_some() && smoke_seconds.is_none() {
            return CliRequest::Invalid {
                reason: "--capture-dir is where --mission --smoke-seconds <n> writes its \
 artifacts; an ordinary mission run writes none"
                    .to_string(),
            };
        }
        let smoke = match smoke_seconds {
            Some(seconds) if seconds < MIN_MISSION_SMOKE_SECONDS => {
                return CliRequest::Invalid {
                    reason: format!(
                        "--smoke-seconds must be at least {MIN_MISSION_SMOKE_SECONDS}: the \
 scripted mission sequence needs that long to probe the world, stand near every spawned \
 actor, reach the trigger, fire the guns and launch a rocket"
                    ),
                };
            }
            Some(seconds) => Some(SmokeRequest {
                seconds,
                capture_dir: capture_dir
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_MISSION_CAPTURE_DIR)),
            }),
            None => None,
        };
        return CliRequest::Mission(MissionRequest {
            cs_path,
            mission,
            smoke,
            smoke_no_teleport,
            dev_overlay,
            dev_keys,
            smoke_engagement,
            smoke_failure,
            smoke_crash,
            smoke_trucks,
            smoke_convoy_kill,
            smoke_success,
            from_start_flow,
            joystick,
            smoke_start_flow,
        });
    }
    if playtest {
        if synthetic || headless || ticks.is_some() || trace.is_some() || seed.is_some() {
            return CliRequest::Invalid {
                reason: "--playtest is the windowed development playtest; it does not \
 combine with --synthetic, --headless, --ticks, --trace or --seed"
                    .to_string(),
            };
        }
        if smoke_seconds.is_none() && capture_dir.is_some() {
            return CliRequest::Invalid {
                reason: "--capture-dir is where --playtest --smoke-seconds <n> writes its \
 artifacts; the interactive playtest writes none"
                    .to_string(),
            };
        }
        let retail = match (cs_path, world, aircraft) {
            (Some(cs_path), world, aircraft) => {
                match RetailRequest::new(cs_path, world.as_deref(), aircraft.as_deref()) {
                    Ok(request) => Some(request),
                    Err(reason) => return CliRequest::Invalid { reason },
                }
            }
            (None, None, None) => None,
            (None, _, _) => {
                return CliRequest::Invalid {
                    reason: "--world and --aircraft select original content, which needs \
 --cs-path <dir> (the plain --playtest is the synthetic scene)"
                        .to_string(),
                };
            }
        };
        let smoke = match smoke_seconds {
            Some(seconds) if seconds < MIN_SMOKE_SECONDS => {
                return CliRequest::Invalid {
                    reason: format!(
                        "--smoke-seconds must be at least {MIN_SMOKE_SECONDS}: the scripted \
 sequence needs that long to pitch, roll, yaw, pause, reset and reach the obstacle"
                    ),
                };
            }
            Some(seconds) => Some(SmokeRequest {
                seconds,
                capture_dir: capture_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_CAPTURE_DIR)),
            }),
            None => None,
        };
        return CliRequest::Playtest(PlaytestRequest { smoke, retail });
    }
    if let Some(cs_path) = &cs_path
        && world.is_none()
        && aircraft.is_none()
        && !synthetic
        && !headless
        && ticks.is_none()
        && trace.is_none()
        && seed.is_none()
        && !smoke_no_teleport
        && !dev_overlay
        && !dev_keys
        && !smoke_engagement
        && !smoke_failure
        && !smoke_crash
        && !smoke_trucks
        && !smoke_convoy_kill
        && !smoke_success
        && !smoke_start_flow
    {
        if capture_dir.is_some() && smoke_seconds.is_none() {
            return CliRequest::Invalid {
                reason: "--capture-dir is where --cs-path <dir> --smoke-seconds <n> writes \
 its artifacts; the interactive start flow writes none"
                    .to_string(),
            };
        }
        let resume = match resume_start_flow.as_deref() {
            None => None,
            Some(key) => match MissionEnd::from_key(key) {
                Some(end) => Some(end),
                None => {
                    return CliRequest::Invalid {
                        reason: format!(
                            "--resume-start-flow expects `cabin` or `abandoned`, found {key:?}"
                        ),
                    };
                }
            },
        };
        // The owner-command smoke (#1832 `PARITY5-START-FLOW.04`): the given
        // directory is the chain's root; each leg's own subdirectory is picked
        // when the leg runs — `front-end/` or `resumed/` for the windowed
        // front end (`run_start_flow`), `mission/` and `replay/` on the
        // command lines `handover_args` writes.
        let smoke = match smoke_seconds {
            Some(seconds) if seconds < MIN_MISSION_SMOKE_SECONDS => {
                return CliRequest::Invalid {
                    reason: format!(
                        "--smoke-seconds must be at least {MIN_MISSION_SMOKE_SECONDS}: the \
 start-flow smoke's mission legs are the mission smoke's own scripted run, which needs \
 that long to fly, pause and leave"
                    ),
                };
            }
            Some(seconds) => Some(SmokeRequest {
                seconds,
                capture_dir: capture_dir
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_FRONT_END_CAPTURE_DIR)),
            }),
            None => None,
        };
        return CliRequest::StartFlow(StartFlowRequest {
            cs_path: cs_path.clone(),
            resume,
            smoke,
        });
    }
    if cs_path.is_some() || world.is_some() || aircraft.is_some() {
        return CliRequest::Invalid {
            reason: "--cs-path, --world and --aircraft select the original-assets \
 --playtest; --mission needs no --world or --aircraft"
                .to_string(),
        };
    }
    if smoke_seconds.is_some() || capture_dir.is_some() {
        return CliRequest::Invalid {
            reason: "--smoke-seconds and --capture-dir belong to --playtest or to --mission"
                .to_string(),
        };
    }
    if seed.is_some() && !synthetic {
        return CliRequest::Invalid {
            reason: "--seed is the root seed of a --synthetic run; no other \
 run mode consumes a seed yet"
                .to_string(),
        };
    }
    if ticks.is_some() && !synthetic {
        return CliRequest::Invalid {
            reason: "--ticks counts simulation ticks for --synthetic; \
 what this stage can run is --synthetic --headless --ticks <n>"
                .to_string(),
        };
    }
    if trace.is_some() && !synthetic {
        return CliRequest::Invalid {
            reason: "--trace records a --synthetic --headless run; no other \
 request writes a trace yet"
                .to_string(),
        };
    }
    if headless && !synthetic {
        return CliRequest::Invalid {
            reason: "--headless selects the headless variant of --synthetic; \
 there is no other headless run mode yet"
                .to_string(),
        };
    }
    if !synthetic {
        return CliRequest::Unsupported { args };
    }
    if !headless {
        return CliRequest::Invalid {
            reason: "--synthetic requires --headless: this workspace stage \
 has no window, renderer or GPU path, so a windowed run would have to fake a \
 scene"
                .to_string(),
        };
    }
    let Some(ticks) = ticks else {
        return CliRequest::Invalid {
            reason: "--synthetic --headless requires --ticks <n>; the run \
 must name the tick count it ends at"
                .to_string(),
        };
    };

    CliRequest::Synthetic(SyntheticRequest { ticks, trace, seed })
}

/// Usage text printed on stdout for `--help`; always exit 0.
///
/// It must keep working without a GPU, a window or a retail installation, so
/// it documents only what this workspace stage really runs.
pub const HELP_TEXT: &str = "\
cs — the Crimson Skies application (2000 PC original-data reimplementation)

USAGE
    cs [OPTIONS]

OPTIONS
    -h, --help   Print this help text and exit 0
    -V, --version   Print the version and exit 0
    --hd <dir>   Draw textures from the HD pack in <dir> (the unextracted
        crimson_hd_chosen_*_textures.zip files of crimson-texture-upscale)
        where it has a replacement; works with every run mode

SYNTHETIC SMOKE
    --synthetic --headless --ticks <n>
        Run the asset-free SYNTHETIC development scene for exactly <n> fixed
        simulation ticks at 64 Hz, without a window, a GPU or an installation
    --trace <file>
        Additionally record the run as JSON Lines: a header line, then one
        sample for each tick from 0 through <n>. The run fails if the file
        cannot be written
    --seed <u64>
        Root seed of the run. It varies only the synthetic body's lateral
        position (within +/- 2 m) and lateral velocity (within +/- 4 m/s);
        every other property of the fixture stays as documented. Omitting it
        keeps the canonical fixture. The trace header records the seed, and
        --seed without --synthetic is rejected

WINDOWED PLAYTEST (development, not original)
    --playtest
        Open a real window with the DEVELOPMENT PLAYTEST / SYNTHETIC SCENE /
        UNCALIBRATED FLIGHT scene and fly the production flight model with the
        keyboard: W/S pitch, Q/E roll, A/D yaw, Left Shift / F throttle,
        R reset, Esc pause/resume, F10 quit. Needs no installation and no GPU
        capture; it is never original M01. See docs/PLAYTEST.md
    --playtest --cs-path <dir> [--world c1c] [--aircraft bloodhawk]
        The same flight loop over ORIGINAL assets read from the installation at
        <dir> (read-only): one documented area of the c1c world with colliders
        derived from its drawn triangles, and the original bloodhawk fuselage
        mesh as the player aircraft, flown by the original fixed-wing law and
        its imported pbloodhawk parameters (OWNER-STATIC-2026-10-08, static
        evidence still uncalibrated against an original run #358), labelled
        ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING /
        ORIGINAL FLIGHT LAW (OWNER-STATIC-2026-10-08, UNCALIBRATED AGAINST AN
        ORIGINAL RUN #358). A missing or unusable <dir> exits
        non-zero; it never falls back to the synthetic scene
    --playtest --smoke-seconds <n> [--capture-dir <dir>]
        Run the scripted, deterministic smoke (n >= 20 simulated seconds) in
        the same window path: it injects keys through the real input path,
        saves framebuffer PNGs, a trace and report.json to <dir> (default
        private/playtest), exits, and fails if a check fails

    --cs-path <dir> --mission <id> [--smoke-seconds <n> [--capture-dir <dir>]]
        Launch one original mission (only M01 is declared). The launch reads
        the installation's own campaign binding and measures the mission's
        dependency closure first; a closure that is not satisfied exits
        non-zero naming every missing surface, and never falls back to
        synthetic content. CS_CAPABILITIES must list audio (e.g.
        CS_CAPABILITIES=retail,gpu,audio); that is checked before the slow
        closure measurement. CS_ENGINE_IMAGE=<path to the owner's
        crimson.decrypted.exe, outside <dir>> is optional: unset, the player's
        airframe and start pose come from the committed owner-static table
        data/owner-static/engine-facts.toml. Startup takes about a minute.
        M01 shows the original world, sky and weather, its actors and AI,
        weapons (Space fires the guns, X a rocket; the original's default keys, F1 lists them), radio dialogue and
        music, the objective HUD and the mission triggers. See
        docs/PLAYTEST.md, section 'Original mission M01'
    --mission <id> --dev-overlay
        Also draw the developer overlay (telemetry, key help, hostile list,
        designed-layout captions). Off by default: the mission shows only the
        original-style cockpit HUD.
    --mission <id> --dev-keys
        Fly with the developer key layout (W/S Q/E A/D, Left Shift / F
        throttle, R reset, F10 quit) instead of the original game's default
        controls.
    --mission <id> --smoke-seconds <n> [--smoke-no-teleport | --smoke-engagement | --smoke-failure | --smoke-crash | --smoke-trucks | --smoke-convoy-kill | --smoke-success] [--capture-dir <dir>]
        The same windowed launch driven as a scripted smoke (n >= 45
        simulated seconds): it injects keys through the real input path,
        places the player near each spawned actor, saves framebuffer PNGs and
        report.json to <dir> (default private/mission-smoke), and fails —
        with a non-zero exit — when the player could not see, hear or shoot
        the mission. A scripted run is never human_play.
        --smoke-no-teleport flies the measured start for the whole run,
        never moves the player and judges only wingmen_near_player: each
        wingman's distance from the player at t+1, 5, 10, 20 and 30 s
        --smoke-engagement flies the measured start with no input for the
        whole --smoke-seconds budget, never moves the player and judges only
        hostiles_damage_idle_player: whether the mission's hostile aircraft
        lowered the player's hull
        --smoke-failure flies the measured start, then destroys the player
        through the session's damage authority (a scripted test input, not
        the original's failure) and judges the journal's debrief screen the
        failed run opens: its text rows, buttons and that the run stays open
        --smoke-crash places the player in a dive onto the terrain (a test
        placement), lets the production terrain-contact rule crash it and
        judges the crash flow: contact sparks and fire, the detached camera
        with 'Fatal Crash!', and the Mission Failed screen after a key
        --smoke-trucks places the player on each declared objective stop
        (test placements) until the mission's own script wakes the convoy
        trucks, holds the player in their range and judges the turret fire
        orders, the hull drop and framebuffer screenshots of the trucks
        --smoke-convoy-kill places the player in front of the bridge convoy
        (a test placement), presses the guns key until the damage authority
        has destroyed every truck and judges the observation rows and
        framebuffer screenshots of the intact and the destroyed convoy
        --smoke-success places the player inside OBJECTIVE12's radius of
        the shipwreck site (a test placement), flies the drop-off landing,
        shoots the Kestrels and a hydrogen tank, docks, and judges which
        objective blocks the mission host reported completed and the terminal
        it settled

    --mission <id> [--joystick <vid:pid|name>] [--force-feedback-gain <percent>]
        A connected HID joystick flies too (SIDEWINDER-FFB2): stick X roll,
        Y pitch, twist yaw, throttle lever throttle (the original's axis
        assignment), its buttons as the original's joystick column (button 1
        guns, 2 rockets, F1 lists them), the hat looks around. A USB PID
        force-feedback stick (Microsoft SideWinder Force Feedback 2) plays a
        designed centring spring, stall buffet, gun, hit and impact feel at
        25% strength unless --force-feedback-gain (0-100) says otherwise;
        effects stop while paused or unfocused and on exit. --joystick picks
        one device; the first is used otherwise
    --mission <id> --no-joystick | --no-force-feedback
        Leave host joysticks alone, or read the stick but never drive its
        motors

JOYSTICK CHECK
    --joystick-probe [--joystick <vid:pid|name>] [--probe-seconds <n>] [--save-descriptor <file>]
        List the host's HID joysticks, then show one's raw axes, buttons and
        hat live for <n> seconds (default 10): what the game will read. Says
        whether its report descriptor matches the bundled SideWinder Force
        Feedback 2 capture and whether it offers USB PID force feedback, and
        names the failure when the device cannot be opened. Sends no force
    --joystick-probe --test-force-feedback [--force-feedback-gain <percent>]
        Also play a test sequence on a USB PID stick: centring spring,
        damper, a push to each side, a buffet and a gun rattle, at 25%
        strength unless asked otherwise. Hold the grip loosely

START FLOW (windowed)
    --cs-path <dir>
        Open the original main menu in a window, with the front-end state
        machine deciding which moves the buttons may make: Preferences and
        Credits open, Quit ends, and Esc steps back only where the original
        binds it (Credits). A screen whose presenter is not built yet (the
        Campaign pilot page, the cabin) is refused on stderr with its task,
        never replaced. Reaching the briefing hands over to
        `--cs-path <dir> --mission M01`
    --cs-path <dir> --smoke-seconds <n> [--capture-dir <dir>]
        The owner-command start-flow smoke (n >= 45, Rally #1832): a chain of
        process images scripts the whole front end and a flight — main menu,
        pilot selection, intro movie, cabin, briefing, Flight Check, launch
        cinematic, a short flight, the pause map's Quit, the cabin again and
        a replayed mission — each leg writing framebuffer PNGs and
        report.json under <dir>/front-end, <dir>/mission, <dir>/resumed and
        <dir>/replay (the root defaults to private/front-end). A screen or a
        frame that never arrives fails the run non-zero, named. A scripted
        run is never human_play
    --mission <id> --smoke-seconds <n> --smoke-start-flow [--capture-dir <dir>]
        One mission leg of the same smoke — the flag the chain's own
        hand-overs carry; it is not meant to be typed by hand

ORIGINAL FRONT END (windowed)
    --cs-path <dir> --front-end
        Open the original front end's own layout sections (ASSETS/LAYOUT.CSV
        of crimson.rof) in a window: the section's panes and 4-state buttons
        drawn from the original graphics at their authored places on the
        800x600 surface, aspect-fit. Up/Down moves button focus, Enter
        presses it, Esc steps back or quits; the pointer hovers and presses.
        A button does only what the original data states (its ScriptToExe
        section, terminate, or its screen script's activate arm); anything
        else is drawn and its press recorded, never guessed. The entry
        section is MainMenu; Campaign and the other reachable sections use
        the same presenter
    --cs-path <dir> --front-end --smoke-seconds <n> [--capture-dir <dir>]
        The same windowed front end driven as a scripted smoke (n >= 8
        simulated seconds): hover and press Campaign, Esc back, focus-walk
        to Quit, press it. Framebuffer PNGs and report.json go to <dir>
        (default private/front-end); the run fails when a framebuffer is
        empty or a step did not happen. A scripted run is never human_play

`--help` and `--version` read no environment variable, open no installation
and start no asset discovery: they succeed without a GPU and without a retail
installation.

This stage runs no window and no renderer, so --synthetic must be combined
with --headless. The remaining run modes of docs/contracts/CLI-EVIDENCE.md
(--input-replay, --cam, --screenshot,
--profile-dir) are not implemented here yet and exit 2.
";

/// Version line printed on stdout for `--version`; always exit 0.
pub fn version_text() -> String {
    format!("cs {}", env!("CARGO_PKG_VERSION"))
}

/// Diagnostic for a call with no arguments, printed on stderr.
pub fn missing_input_message() -> String {
    "cs: no input specified; expected --synthetic --headless --ticks <n> or \
 --help (see docs/contracts/CLI-EVIDENCE.md)"
        .to_string()
}

/// Diagnostic for a vector that names no supported request, printed on
/// stderr.
pub fn unsupported_message(args: &[String]) -> String {
    format!(
        "cs: unsupported arguments [{}]; run cs --help for the request \
 forms this stage implements (docs/contracts/CLI-EVIDENCE.md)",
        args.join(" ")
    )
}

/// Diagnostic for a recognized flag given an unusable value, printed on
/// stderr. The reason names the offending flag.
pub fn invalid_message(reason: &str) -> String {
    format!("cs: {reason}")
}
