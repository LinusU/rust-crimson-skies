//! `cs_xtask` — the workspace's reproducible testing and packaging gates.
//!
//! Two commands, both local (the owner's F00-C note keeps task-specific
//! discovery out of CI):
//!
//! * `test-select --prefix <prefix>` runs the task's positive test selection
//!   through [`test_select::run_gate`] and fails when it selects nothing,
//!   when anything fails or when a discovered test does not run alone with
//!   `--exact`.
//! * `verify-ci` checks that the owner-maintained workflow still runs the
//!   workspace gates ([`ci`]).
//!
//! Exit codes: 0 gate passed, 1 the gate failed, 2 the request itself was
//! invalid. Failures are printed on stderr, never returned as success.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_xtask::ci;
use cs_xtask::test_select;

/// The gate ran and passed.
const EXIT_OK: u8 = 0;
/// The gate ran and failed (or the workspace it pointed at is unusable).
const EXIT_GATE_FAILED: u8 = 1;
/// The command line named no valid request.
const EXIT_USAGE: u8 = 2;

const USAGE: &str = "\
cs_xtask — reproducible testing, coverage and packaging gates

USAGE
    cs_xtask <command> [options]

COMMANDS
    test-select --prefix <prefix> [--workspace-root <dir>]
        Run `cargo test --workspace --locked -- <prefix> --include-ignored`,
        require that it selects at least one test and that none fail, then
        re-run every discovered test alone with `--exact`.
    verify-ci [--workspace-root <dir>]
        Check that .github/workflows/ci.yml still runs cargo fmt, cargo
        clippy with -D warnings and the workspace test suite.

OPTIONS
    --prefix <prefix>       Task test prefix, e.g. accept_f00_c_
    --workspace-root <dir>  Workspace to run in (default: current directory)
    -h, --help              Print this help text and exit 0
    -V, --version           Print the version and exit 0
";

/// Options shared by both commands.
struct Options {
    prefix: Option<String>,
    workspace_root: PathBuf,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return ExitCode::from(EXIT_USAGE);
    };

    match command {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            ExitCode::from(EXIT_OK)
        }
        "-V" | "--version" => {
            println!("cs_xtask {}", env!("CARGO_PKG_VERSION"));
            ExitCode::from(EXIT_OK)
        }
        "test-select" => run_test_select(&args[1..]),
        "verify-ci" => run_verify_ci(&args[1..]),
        other => {
            eprintln!("cs-xtask: unknown command {other:?}");
            eprint!("{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Parses `--prefix`, `--workspace-root` and rejects anything else.
fn parse_options(args: &[String], allow_prefix: bool) -> Result<Options, String> {
    let mut prefix = None;
    let mut workspace_root = PathBuf::from(".");
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--prefix" if allow_prefix => {
                if prefix.is_some() {
                    return Err("--prefix was given twice".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("--prefix needs a value".to_string());
                };
                prefix = Some(value.clone());
                index += 2;
            }
            "--workspace-root" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--workspace-root needs a value".to_string());
                };
                workspace_root = PathBuf::from(value);
                index += 2;
            }
            option => return Err(format!("unknown option {option:?}")),
        }
    }
    Ok(Options {
        prefix,
        workspace_root,
    })
}

/// Reports an unusable workspace root without letting cargo fail obscure it.
fn require_workspace(root: &Path) -> Result<(), String> {
    if root.join("Cargo.toml").is_file() {
        Ok(())
    } else {
        Err(format!(
            "{} is not a workspace root (no Cargo.toml)",
            root.display()
        ))
    }
}

fn run_test_select(args: &[String]) -> ExitCode {
    let options = match parse_options(args, true) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    let Some(prefix) = options.prefix else {
        return usage_error("test-select requires --prefix <prefix>");
    };
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    match test_select::run_gate(&options.workspace_root, &prefix) {
        Ok(report) => {
            let selection = &report.selection;
            for name in &selection.tests {
                println!("selected {name}");
            }
            println!(
                "test-select: prefix {prefix:?} selected {} test(s) ({} passed, \
 {} failed, {} ignored)",
                selection.tests.len(),
                selection.passed,
                selection.failed,
                selection.ignored
            );
            println!(
                "test-select: {} test(s) re-ran alone with --exact and passed",
                report.exact_verified
            );
            ExitCode::from(EXIT_OK)
        }
        Err(error) => gate_failed(&error.to_string()),
    }
}

fn run_verify_ci(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    match ci::verify_workspace_workflow(&options.workspace_root) {
        Ok(()) => {
            println!(
                "verify-ci: {} runs cargo fmt, cargo clippy with -D warnings and \
 the workspace test suite",
                ci::WORKFLOW_PATH
            );
            ExitCode::from(EXIT_OK)
        }
        Err(error) => gate_failed(&error.to_string()),
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("cs-xtask: {message}");
    ExitCode::from(EXIT_USAGE)
}

fn gate_failed(message: &str) -> ExitCode {
    eprintln!("cs-xtask: {message}");
    ExitCode::from(EXIT_GATE_FAILED)
}
