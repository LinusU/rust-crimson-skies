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

use crate::run::SyntheticRequest;

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
pub const EXIT_INVALID_INPUT: u8 = 2;

/// Exit code for a runtime failure: anything that is not invalid input (2),
/// failed validation (3) or a missing capability (4) per
/// `docs/contracts/CLI-EVIDENCE.md`. A failure is never reported as zero.
pub const EXIT_RUNTIME_FAILURE: u8 = 1;

/// What the caller asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliRequest {
    /// `-h`/`--help`: print [`HELP_TEXT`] on stdout and exit 0.
    Help,
    /// `-V`/`--version`: print [`version_text`] on stdout and exit 0.
    Version,
    /// `--synthetic --headless --ticks <n> [--trace <file>]`: run the
    /// asset-free `SYNTHETIC` scene for exactly that many fixed ticks.
    Synthetic(SyntheticRequest),
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
/// `--synthetic --headless --ticks <n> [--trace <file>]` request; a vector
/// that is unknown or structurally incomplete becomes
/// [`CliRequest::Unsupported`], and one that is complete but unusable (a
/// non-numeric tick count, `--synthetic` without `--headless`) becomes
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
    let mut unknown = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].clone();
        let mut values = 0;
        match arg.as_str() {
            "--synthetic" => synthetic = true,
            "--headless" => headless = true,
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

    CliRequest::Synthetic(SyntheticRequest { ticks, trace })
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

`--help` and `--version` read no environment variable, open no installation
and start no asset discovery: they succeed without a GPU and without a retail
installation.

This stage runs no window and no renderer, so --synthetic must be combined
with --headless. The remaining run modes of docs/contracts/CLI-EVIDENCE.md
(--cs-path <dir> --mission <id>, --input-replay, --cam, --screenshot,
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
