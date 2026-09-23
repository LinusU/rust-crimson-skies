//! `cs` — the Crimson Skies application binary.
//!
//! F00-A ships only the workspace bootstrap guard: `--help`/`--version`
//! arrive with F00-B and the synthetic/mission run modes with F00-C (see
//! `specs/F00-workspace-toolchain-and-first-executable.md`). Whatever this
//! binary cannot do, it reports on stderr with a nonzero exit code — per
//! `docs/contracts/CLI-EVIDENCE.md` a failure is never returned as success.

use std::process::ExitCode;

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
const EXIT_INVALID_INPUT: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "cs: no input specified; expected --synthetic or --cs-path <dir> --mission <id> \
             (see docs/contracts/CLI-EVIDENCE.md)"
        );
        return ExitCode::from(EXIT_INVALID_INPUT);
    }
    eprintln!(
        "cs: unsupported arguments [{}]; this workspace stage implements no run modes yet",
        args.join(" ")
    );
    ExitCode::from(EXIT_INVALID_INPUT)
}
