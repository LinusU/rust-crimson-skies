//! `cs-inspect` — the command-line inspection binary.
//!
//! `--help` and `--version` exit 0 without a GPU or a retail installation and
//! without starting any asset discovery (F00 non-negotiable behavior 3). The
//! `inventory` command (F02-C) runs production discovery over the selected
//! installation and writes the inventory and dependency-impact report, and
//! the `audit` command (F02-D) classifies every inventoried file and runs
//! the full-content readiness check, and the `resolve` command (F04-C)
//! resolves one asset key in a mounted content session and reports its
//! trace, and the `zbd-audit` command (F06-D) lists every ZBD container
//! and member with a strict status. The remaining subcommands from `docs/contracts/CLI-EVIDENCE.md`
//! (`catalog`, `closure`, `scripts`, `handling`) arrive with later tasks.
//! Until then the binary refuses invalid input with a nonzero exit code and
//! a diagnostic naming the missing command — a failure is never returned as success.

use std::process::ExitCode;

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
const EXIT_INVALID_INPUT: u8 = 2;

const HELP_TEXT: &str = "\
cs-inspect — inspect a Crimson Skies installation

USAGE
    cs-inspect [OPTIONS] <COMMAND>

OPTIONS
    -h, --help       Print this help text and exit 0
    -V, --version    Print the version and exit 0

COMMANDS
    inventory [--cs-path <dir>] [--out <file>]
        Inventory the selected installation (--cs-path wins over
        CS_GAME_DIR) and write the JSON inventory and dependency-impact
        report to --out, or stdout without it.

    audit [--cs-path <dir>] [--scope all] [--strict] [--out <file>]
        Audit the selected installation: classify every inventoried file
        and run the full-content readiness check. Exits 0 when the
        installation passes, 3 when it does not.

    resolve [--cs-path <dir>] --asset <namespace>:<path> [--variant <v>]
            [--world <group>] [--locale <l>] [--mission <m>] [--out <file>]
            [--export-dir <dir>]
        Mount the installation into one content session and resolve the key
        (namespaces: install, world). The JSON report holds the span, every
        ordered attempt and the precedence status. --export-dir writes the
        resolved member into a private directory outside the installation.
        Exits 0 when resolved, 3 when not found or ambiguous.

    zbd-audit [--cs-path <dir>] [--strict] [--out <file>]
        Audit every ZBD container of the installation family by family:
        route it, read its own member index and give every member a row
        (decoded, readable or failed). Exits 3 when a container or member
        is corrupt, and with --strict also when any content is left
        uninterpreted. Never claims playability.

    catalog  closure  scripts  handling
        Not implemented in this workspace stage; they are documented in
        docs/contracts/CLI-EVIDENCE.md.

`--help` and `--version` read no environment variable and open no
installation.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP_TEXT}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" => {
                println!("cs-inspect {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    match args.first().map(String::as_str) {
        None => {
            eprintln!(
                "cs-inspect: missing command; expected one of inventory, audit, resolve, \
                 zbd-audit, catalog, closure, scripts, handling (see docs/contracts/CLI-EVIDENCE.md)"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
        Some("inventory") => cs_inspect::install::inventory_command(&args[1..]),
        Some("audit") => cs_inspect::install::audit_command(&args[1..]),
        Some("resolve") => cs_inspect::resolve::resolve_command(&args[1..]),
        Some("zbd-audit") => cs_inspect::zbd::zbd_audit_command(&args[1..]),
        Some(command) => {
            eprintln!(
                "cs-inspect: unsupported command {command:?}; this workspace stage implements \
                 only `inventory`, `audit`, `resolve` and `zbd-audit`"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
    }
}
