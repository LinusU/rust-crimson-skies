//! `cs` — the Crimson Skies application binary.
//!
//! The request comes from [`cs_app::cli::parse`]: `--help`/`--version` exit 0
//! on stdout without touching the environment or the retail installation
//! (F00-B, acceptance case AC02). Whatever this binary cannot do, it reports
//! on stderr with a nonzero exit code — per `docs/contracts/CLI-EVIDENCE.md`
//! a failure is never returned as success. Run modes arrive with F00-C (see
//! `specs/F00-workspace-toolchain-and-first-executable.md`).

use std::process::ExitCode;

use cs_app::cli::{self, CliRequest};

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        CliRequest::Help => {
            print!("{}", cli::HELP_TEXT);
            ExitCode::SUCCESS
        }
        CliRequest::Version => {
            println!("{}", cli::version_text());
            ExitCode::SUCCESS
        }
        CliRequest::MissingInput => {
            eprintln!("{}", cli::missing_input_message());
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
        CliRequest::Unsupported { args } => {
            eprintln!("{}", cli::unsupported_message(&args));
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
    }
}
