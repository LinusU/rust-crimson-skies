//! `cs-inspect` — the command-line inspection binary.
//!
//! F00-A ships only the workspace bootstrap guard: the subcommands from
//! `docs/contracts/CLI-EVIDENCE.md` (`inventory`, `catalog`, `resolve`,
//! `closure`, `scripts`, `handling`, `audit`) arrive with later tasks. Until
//! then the binary refuses invalid input with a nonzero exit code and a
//! diagnostic naming the missing command — a failure is never returned as
//! success.

use std::process::ExitCode;

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
const EXIT_INVALID_INPUT: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "cs-inspect: missing command; expected one of inventory, catalog, resolve, closure, \
             scripts, handling, audit (see docs/contracts/CLI-EVIDENCE.md)"
        );
        return ExitCode::from(EXIT_INVALID_INPUT);
    }
    eprintln!(
        "cs-inspect: unsupported arguments [{}]; this workspace stage implements no commands yet",
        args.join(" ")
    );
    ExitCode::from(EXIT_INVALID_INPUT)
}
