//! `cs_xtask` — the workspace's reproducible testing and packaging gates.
//!
//! Seven commands, all local (the owner's F00-C note keeps task-specific
//! discovery out of CI):
//!
//! * `test-select --prefix <prefix>` runs the task's positive test selection
//!   through [`test_select::run_gate`] and fails when it selects nothing,
//!   when anything fails or when a discovered test does not run alone with
//!   `--exact`.
//! * `verify-ci` checks that the owner-maintained workflow still runs the
//!   workspace gates ([`ci`]).
//! * `verify-bootstrap` checks the whole platform bootstrap
//!   ([`bootstrap`]): every required workspace member is listed with a real
//!   manifest, the Bevy/Avian/toolchain pins are frozen, and the CI gates
//!   are still there.
//! * `verify-ci-budget` checks that the profiles CI builds under do not emit
//!   full DWARF, the footprint that ran the runner out of disk (task #430,
//!   [`budget`]).
//! * `verify-target-dir` checks that the effective `CARGO_TARGET_DIR` is
//!   private to this worktree ([`target_dir`]), so concurrent agent builds
//!   cannot reuse each other's artifacts (task #383); that it no longer holds
//!   artifacts a removed worktree produced (task #433); and that it does not
//!   hold artifacts a different worktree that is still there produced (task
//!   #440).
//! * `verify-package` reads a candidate release manifest and fails when it
//!   carries proprietary content, an unsafe member path or is missing a
//!   required notice ([`package`], F61-A).
//! * `corpus manifest` prints the declared F62-A corpus contract as JSON and
//!   `corpus audit` checks the synthetic/private separation rules against the
//!   real tracked file list ([`corpus`]).
//!
//! Exit codes: 0 gate passed, 1 the gate failed, 2 the request itself was
//! invalid. Failures are printed on stderr, never returned as success.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_xtask::bootstrap;
use cs_xtask::budget;
use cs_xtask::ci;
use cs_xtask::corpus;
use cs_xtask::package;
use cs_xtask::target_dir;
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
    verify-bootstrap [--workspace-root <dir>]
        Check the whole platform bootstrap: every workspace member required
        by the F00 deliverable is listed in [workspace] members with a real
        [package] manifest, Cargo.lock and rust-toolchain.toml still freeze
        the Bevy 0.19 / Avian3d 0.7 pair and an exact toolchain, and the CI
        workflow keeps its gates.
    verify-ci-budget [--workspace-root <dir>]
        Check that the profiles CI builds under (dev, test, bench) do not
        emit full DWARF. Full DWARF for the Bevy dependency graph is the
        footprint that ran the runner out of disk and killed the cs_app
        doctest link with SIGBUS (task #430).
    verify-target-dir [--workspace-root <dir>]
        Check that the effective CARGO_TARGET_DIR is private to this
        worktree and free of artifacts another checkout left behind: a
        directory shared between checkouts lets concurrent builds reuse each
        other's artifacts; one that outlived its worktree lets a later build
        reuse artifacts whose CARGO_MANIFEST_DIR names a path that no longer
        exists; and one that still holds a live sibling worktree's artifacts
        lets cargo run that worktree's binary. A green or red test run would
        not be evidence about this tree in any of them (tasks #383, #433 and
        #440).
    verify-package --manifest <file> [--workspace-root <dir>]
        Read a candidate release manifest (F61-A) and refuse it when it
        carries proprietary content, a member path that could escape the
        archive, a member nothing classifies, or a missing required notice or
        engine binary.
    corpus manifest
        Print the declared F62-A corpus contract as JSON: the known
        container entrypoints with their truncation oracles and boundary
        kinds, every corpus entry with its synthetic/private/regression
        class, and the real counts.
    corpus audit [--workspace-root <dir>] [--private-root <dir>]
        Check the separation contract: the manifest is internally
        consistent, no tracked file lives under a private-only prefix, and
        every committed synthetic fixture is tracked. With --private-root
        (the read-only original installation or a private corpus dir), each
        private selector is enumerated and fingerprinted; without it the
        private suite is reported unavailable rather than passed.

OPTIONS
    --prefix <prefix>       Task test prefix, e.g. accept_f00_c_
    --workspace-root <dir>  Workspace to run in (default: current directory)
    --manifest <file>       verify-package only: candidate release manifest
                            to scan
    --private-root <dir>    corpus audit only: private corpus root,
                            e.g. the read-only original installation
    -h, --help              Print this help text and exit 0
    -V, --version           Print the version and exit 0
";

/// Options shared by the subcommands.
struct Options {
    prefix: Option<String>,
    workspace_root: PathBuf,
    manifest: Option<PathBuf>,
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
        "verify-bootstrap" => run_verify_bootstrap(&args[1..]),
        "verify-ci-budget" => run_verify_ci_budget(&args[1..]),
        "verify-target-dir" => run_verify_target_dir(&args[1..]),
        "verify-package" => run_verify_package(&args[1..]),
        "corpus" => run_corpus(&args[1..]),
        other => {
            eprintln!("cs-xtask: unknown command {other:?}");
            eprint!("{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Parses `--prefix`, `--workspace-root`, `--manifest` and rejects anything
/// else. `allow_prefix` gates `--prefix` to `test-select` alone, so a typo
/// aimed at another subcommand is an error rather than a silently ignored flag.
fn parse_options(
    args: &[String],
    allow_prefix: bool,
    allow_manifest: bool,
) -> Result<Options, String> {
    let mut prefix = None;
    let mut workspace_root = PathBuf::from(".");
    let mut manifest = None;
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
            "--manifest" if allow_manifest => {
                if manifest.is_some() {
                    return Err("--manifest was given twice".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("--manifest needs a value".to_string());
                };
                manifest = Some(PathBuf::from(value));
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
        manifest,
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
    let options = match parse_options(args, true, false) {
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
    let options = match parse_options(args, false, false) {
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

/// Runs the platform bootstrap gate: required workspace members with real
/// manifests, frozen pins, intact CI gates — all four checks must pass.
fn run_verify_bootstrap(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false, false) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    match bootstrap::verify_workspace(&options.workspace_root) {
        Ok(report) => {
            println!(
                "verify-bootstrap: {} required workspace members are listed in \
 [workspace] members, each with a [package] manifest",
                bootstrap::REQUIRED_MEMBERS.len()
            );
            println!(
                "verify-bootstrap: pins frozen — bevy {}, avian3d {}, workspace \
 rust-version {}, toolchain {}",
                report.pins.bevy,
                report.pins.avian3d,
                report.pins.rust_version,
                report.pins.toolchain_channel
            );
            println!(
                "verify-bootstrap: {} keeps cargo fmt, cargo clippy with -D warnings \
 and the workspace test suite",
                ci::WORKFLOW_PATH
            );
            ExitCode::from(EXIT_OK)
        }
        Err(error) => gate_failed(&error.to_string()),
    }
}

/// Runs the CI build-footprint gate (task #430): the profiles CI links under
/// must not emit full DWARF.
fn run_verify_ci_budget(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false, false) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    match budget::verify_workspace(&options.workspace_root) {
        Ok(()) => {
            println!(
                "verify-ci-budget: {} keeps [profile.dev] debug at a level that \
does not emit full DWARF for CI's dev, test and bench profiles",
                budget::MANIFEST_PATH
            );
            ExitCode::from(EXIT_OK)
        }
        Err(error) => gate_failed(&error.to_string()),
    }
}

/// Runs the per-worktree target-directory gate (task #383).
fn run_verify_target_dir(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false, false) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    match target_dir::verify_workspace(&options.workspace_root) {
        Ok(dir) => {
            println!(
                "verify-target-dir: {} is private to worktree {}",
                dir.display(),
                options.workspace_root.display()
            );
            ExitCode::from(EXIT_OK)
        }
        Err(error) => gate_failed(&error.to_string()),
    }
}

/// `corpus manifest` prints the declared contract; `corpus audit` checks
/// the separation rules against the real tracked file list and, when
/// `--private-root` is given, resolves the private selectors.
fn run_corpus(args: &[String]) -> ExitCode {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return usage_error("corpus requires a subcommand: manifest | audit");
    };
    match subcommand {
        "manifest" => {
            if args.len() != 1 {
                return usage_error("corpus manifest takes no options");
            }
            print!("{}", corpus::manifest_json());
            ExitCode::from(EXIT_OK)
        }
        "audit" => {
            let mut workspace_root = PathBuf::from(".");
            let mut private_root: Option<PathBuf> = None;
            let mut index = 1;
            while index < args.len() {
                match args[index].as_str() {
                    "--workspace-root" => {
                        let Some(value) = args.get(index + 1) else {
                            return usage_error("--workspace-root needs a value");
                        };
                        workspace_root = PathBuf::from(value);
                        index += 2;
                    }
                    "--private-root" => {
                        let Some(value) = args.get(index + 1) else {
                            return usage_error("--private-root needs a value");
                        };
                        private_root = Some(PathBuf::from(value));
                        index += 2;
                    }
                    option => return usage_error(&format!("unknown option {option:?}")),
                }
            }
            if let Err(error) = require_workspace(&workspace_root) {
                return gate_failed(&error);
            }
            match corpus::audit(&workspace_root, private_root.as_deref()) {
                Ok(report) => {
                    for path in &report.tracked_private_paths {
                        eprintln!("corpus audit: tracked private path {path}");
                    }
                    for path in &report.missing_committed_fixtures {
                        eprintln!("corpus audit: committed fixture {path} is not tracked");
                    }
                    for error in &report.manifest_errors {
                        eprintln!("corpus audit: {error}");
                    }
                    match report.private_availability {
                        corpus::PrivateAvailability::Unavailable => {
                            println!(
                                "corpus audit: private corpus unavailable (no --private-root); \
                                 {} private selector(s) not exercised",
                                report
                                    .private_selectors
                                    .len()
                                    .max(count_private_selectors())
                            );
                        }
                        corpus::PrivateAvailability::Available => {
                            for selector in &report.private_selectors {
                                println!(
                                    "corpus audit: {} matched {} member(s), {} bytes, sha256 {}",
                                    selector.entry,
                                    selector.members,
                                    selector.total_bytes,
                                    corpus::sha256_hex(&selector.fingerprint),
                                );
                            }
                        }
                    }
                    let counts = corpus::manifest_counts();
                    println!(
                        "corpus audit: {} container(s), {} synthetic + {} private + \
                         {} regression entries, {} fuzz target(s)",
                        counts.containers,
                        counts.synthetic,
                        counts.private,
                        counts.regression,
                        counts.fuzz_targets,
                    );
                    if corpus::audit_is_clean(&report) {
                        ExitCode::from(EXIT_OK)
                    } else {
                        ExitCode::from(EXIT_GATE_FAILED)
                    }
                }
                Err(error) => gate_failed(&error.to_string()),
            }
        }
        other => usage_error(&format!("unknown corpus subcommand {other:?}")),
    }
}

fn count_private_selectors() -> usize {
    corpus::entries()
        .iter()
        .filter(|entry| entry.class == corpus::CorpusClass::Private)
        .count()
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("cs-xtask: {message}");
    ExitCode::from(EXIT_USAGE)
}

/// Runs the release-contents gate (F61-A): read a candidate manifest and fail
/// when it may not be released.
fn run_verify_package(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false, true) {
        Ok(options) => options,
        Err(error) => return usage_error(&error),
    };
    let Some(manifest) = options.manifest else {
        return usage_error("verify-package requires --manifest <file>");
    };
    // `--workspace-root` is accepted by every subcommand, so it is checked
    // rather than silently ignored: a gate that reads a flag it ignores cannot
    // be told apart from one that honoured it.
    if let Err(error) = require_workspace(&options.workspace_root) {
        return gate_failed(&error);
    }

    let candidate = match package::read_manifest(&manifest) {
        Ok(candidate) => candidate,
        Err(error) => return gate_failed(&error.to_string()),
    };
    let report = package::scan(&candidate);
    if report.is_releasable() {
        println!(
            "verify-package: {} is releasable — {} member(s), {} byte(s), no proprietary \
             content and every required notice present",
            report.version, report.member_count, report.total_bytes
        );
        ExitCode::from(EXIT_OK)
    } else {
        for line in report.lines() {
            eprintln!("cs-xtask: {line}");
        }
        gate_failed(&format!(
            "{} is not releasable: {} finding(s)",
            manifest.display(),
            report.findings.len()
        ))
    }
}

fn gate_failed(message: &str) -> ExitCode {
    eprintln!("cs-xtask: {message}");
    ExitCode::from(EXIT_GATE_FAILED)
}
