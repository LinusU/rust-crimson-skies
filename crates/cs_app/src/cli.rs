//! Command-line request parsing for the `cs` binary.
//!
//! Parsing is a pure function of the argument vector: it never reads
//! `CS_GAME_DIR`, opens the retail installation, starts asset discovery or
//! touches the file system. That is what makes `--help` and `--version` work
//! without a GPU and without a retail installation (F00 non-negotiable
//! behavior 3, acceptance case AC02 in
//! `specs/F00-workspace-toolchain-and-first-executable.md`).
//!
//! The run modes named in `docs/contracts/CLI-EVIDENCE.md` are parsed and
//! rejected here until the stages that implement them land; a rejected request
//! is reported as a failure with exit code [`EXIT_INVALID_INPUT`], never as
//! success.

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
pub const EXIT_INVALID_INPUT: u8 = 2;

/// What the caller asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliRequest {
    /// `-h`/`--help`: print [`HELP_TEXT`] on stdout and exit 0.
    Help,
    /// `-V`/`--version`: print [`version_text`] on stdout and exit 0.
    Version,
    /// No arguments at all: invalid input, reported on stderr and exit 2.
    MissingInput,
    /// Arguments that name no supported run mode yet: reported on stderr and
    /// exit 2.
    Unsupported { args: Vec<String> },
}

/// Classifies the argument vector left after the program name.
///
/// Recognized flags win in the order they appear; everything else stays an
/// [`CliRequest::Unsupported`] request so later stages can extend the parser
/// without changing the exit-code contract.
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
    CliRequest::Unsupported { args }
}

/// Usage text printed on stdout for `--help`; always exit 0.
pub const HELP_TEXT: &str = "\
cs — the Crimson Skies application (2000 PC original-data reimplementation)

USAGE
    cs [OPTIONS]

OPTIONS
    -h, --help       Print this help text and exit 0
    -V, --version    Print the version and exit 0

`--help` and `--version` read no environment variable, open no installation
and start no asset discovery: they succeed without a GPU and without a retail
installation.

Run modes are not implemented in this workspace stage. The planned interface
(--synthetic, --cs-path <dir> --mission <id>, --headless, --ticks, --trace,
--cam, --input-replay, --screenshot, --profile-dir) is documented in
docs/contracts/CLI-EVIDENCE.md; unsupported arguments exit 2.
";

/// Version line printed on stdout for `--version`; always exit 0.
pub fn version_text() -> String {
    format!("cs {}", env!("CARGO_PKG_VERSION"))
}

/// Diagnostic for a call with no arguments, printed on stderr.
pub fn missing_input_message() -> String {
    "cs: no input specified; expected --synthetic or --cs-path <dir> --mission <id> \
 (see docs/contracts/CLI-EVIDENCE.md)"
        .to_string()
}

/// Diagnostic for arguments that name no supported run mode, printed on
/// stderr.
pub fn unsupported_message(args: &[String]) -> String {
    format!(
        "cs: unsupported arguments [{}]; this workspace stage implements no run modes yet",
        args.join(" ")
    )
}
