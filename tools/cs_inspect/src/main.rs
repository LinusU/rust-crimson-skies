//! `cs-inspect` — the command-line inspection binary.
//!
//! `--help` and `--version` exit 0 without a GPU or a retail installation and
//! without starting any asset discovery (F00 non-negotiable behavior 3). The
//! `inventory` command (F02-C) runs production discovery over the selected
//! installation and writes the inventory and dependency-impact report, and
//! the `audit` command (F02-D) classifies every inventoried file and runs
//! the full-content readiness check, and the `resolve` command (F04-C)
//! resolves one asset key in a mounted content session and reports its
//! trace, and the `rof` command (F05-C) mounts one ROF container into a
//! content session, reports every member it holds, optionally reads
//! and exports one of them and — with `--audit` (F05-D) — reads every
//! member to report both length words of each record and how the
//! container's bytes divide, the `zbd-audit` command (F06-D) lists every
//! ZBD container and member with a strict status, and the `interp`
//! command (F07-B) decodes and validates one INTERP loading-script
//! container and reports its lossless tokens, and with `--plan` reads the
//! same container as a loading plan (F07-C), resolving the registered
//! commands through a content session and reporting every failure with its
//! source offset, and the `texture-audit` command (F08-D) compares every
//! ZBD texture's decode with a pinned reference extraction and can write a
//! private contact sheet. The remaining subcommands from
//! `docs/contracts/CLI-EVIDENCE.md` (`catalog`, `closure`,
//! `scripts`, `handling`) arrive with later tasks.
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

    rof [--cs-path <dir>] --container <spelling> [--member <spelling>]
        [--max-decoded-bytes <n>] [--audit] [--out <file>] [--export-dir <dir>]
        Mount one ROF container of the installation into a content
        session and report every member it holds: spelling, id, stored
        extent, compression bit and digest. --member reads one member
        through the bounded decoder and --export-dir writes its decoded
        bytes into a private directory outside the installation. --audit
        reads every member instead of one and reports both length words of
        each record, the bytes the read produced, the trailing bytes inside
        each stored extent, the stored and decoded digests and how the
        container's bytes divide between its directory blocks and its
        members' stored extents. Exits 0 when mounted, 3 when the
        container, the member or the audit is refused.

    zbd-audit [--cs-path <dir>] [--strict] [--out <file>]
        Audit every ZBD container of the installation family by family:
        route it, read its own member index and give every member a row
        (decoded, readable or failed). Exits 3 when a container or member
        is corrupt, and with --strict also when any content is left
        uninterpreted. Never claims playability.

    interp --file <path> [--out <file>] [--raw] [--plan] [--commands <file>]
           [--cs-path <dir>] [--world <group>]
        Decode and validate one INTERP loading-script container (F07-B), and
        with --plan read it as a loading plan (F07-C): classify every line
        against a command table, resolve the registered commands through a
        mounted content session, and report every failure with its source
        offset and the world it affects. The JSON report holds the header,
        every script's origin and extent, and each line's arguments as
        offsets and byte strings; names and arguments have no established
        encoding, so they are reported as length and hex, never as text.
        --raw adds the unvalidated raw records next to the decoded tokens.
        --commands supplies the command table the workspace does not ship:
        which commands load resources is unmeasured (F07-D). Exits 0 when the
        container decodes with no findings and, with --plan, when the plan is
        complete; 3 when it is refused, has findings or the plan is
        incomplete.

    texture-audit [--cs-path <dir>] --reference <dir> [--sheet-dir <dir>]
                  [--out <file>]
        Decode every ZBD texture through the catalog's upload boundary and
        compare dimensions, coverage, orientation and every texel with the
        pinned reference extraction in --reference (<dir>/<archive>.zip,
        written by `unzbd cs textures`, mech3ax v0.6.0). --sheet-dir writes
        one private contact sheet (TGA) per archive. Exits 0 when no
        unexplained difference remains, 3 otherwise.

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
                "cs-inspect: missing command; expected one of inventory, audit, resolve, rof, \
                 zbd-audit, interp, texture-audit, catalog, closure, scripts, handling (see \
                 docs/contracts/CLI-EVIDENCE.md)"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
        Some("inventory") => cs_inspect::install::inventory_command(&args[1..]),
        Some("audit") => cs_inspect::install::audit_command(&args[1..]),
        Some("resolve") => cs_inspect::resolve::resolve_command(&args[1..]),
        Some("rof") => cs_inspect::rof::rof_command(&args[1..]),
        Some("zbd-audit") => cs_inspect::zbd::zbd_audit_command(&args[1..]),
        Some("interp") => cs_inspect::interp::interp_command(&args[1..]),
        Some("texture-audit") => cs_inspect::textures::texture_audit_command(&args[1..]),
        Some(command) => {
            eprintln!(
                "cs-inspect: unsupported command {command:?}; this workspace stage implements \
                 only `inventory`, `audit`, `resolve`, `rof`, `zbd-audit`, `interp` and \
                 `texture-audit`"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
    }
}
