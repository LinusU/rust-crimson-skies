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

use crate::playtest::smoke::MIN_SMOKE_SECONDS;
use crate::playtest::{DEFAULT_CAPTURE_DIR, PlaytestRequest, RetailRequest, SmokeRequest};
use crate::run::SyntheticRequest;

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
    let mut unknown = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].clone();
        let mut values = 0;
        match arg.as_str() {
            "--synthetic" => synthetic = true,
            "--headless" => headless = true,
            "--playtest" => playtest = true,
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
        return CliRequest::Mission(MissionRequest { cs_path, mission });
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
    if cs_path.is_some() || world.is_some() || aircraft.is_some() {
        return CliRequest::Invalid {
            reason: "--cs-path, --world and --aircraft select the original-assets \
 --playtest; --mission needs no --world or --aircraft"
                .to_string(),
        };
    }
    if smoke_seconds.is_some() || capture_dir.is_some() {
        return CliRequest::Invalid {
            reason: "--smoke-seconds and --capture-dir belong to --playtest".to_string(),
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

    --cs-path <dir> --mission <id>
        Launch one original mission (only M01 is declared). The launch reads
        the installation's own campaign binding and measures the mission's
        dependency closure first; a closure that is not satisfied exits
        non-zero naming every missing surface, and never falls back to
        synthetic content

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
