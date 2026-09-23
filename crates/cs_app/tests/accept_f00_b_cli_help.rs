//! Acceptance scenario F00-B / AC02: `cs --help` with `CS_GAME_DIR` unset
//! exits zero and causes no asset-discovery side effects.
//!
//! The negative cases matter just as much: an implementation that "passes" by
//! always exiting zero, or that starts discovering assets before answering
//! `--help`, must fail these tests. Everything asserted here is observed on
//! the real `cs` binary and on the production parser in `cs_app::cli`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cs_app::cli::{self, CliRequest};

/// A fresh, empty directory inside the workspace `target/`, used to prove the
/// binary writes nothing while answering `--help`.
fn empty_scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("{name}_{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the scratch dir must be reusable");
    }
    fs::create_dir_all(&dir).expect("the scratch dir must be creatable");
    dir
}

fn cs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs"))
}

/// AC02: with `CS_GAME_DIR` unset, `--help` exits 0, prints usage on stdout,
/// prints nothing on stderr and creates no files in an empty working
/// directory.
///
/// Observable failure if the implementation is removed: the F00-A guard
/// treats every argument as unsupported input and exits 2.
#[test]
fn accept_f00_b_help_without_cs_game_dir_exits_zero_without_side_effects() {
    let workdir = empty_scratch_dir("accept_f00_b_help_cwd");

    let output = cs()
        .arg("--help")
        .env_remove("CS_GAME_DIR")
        .current_dir(&workdir)
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--help must exit zero with CS_GAME_DIR unset, got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--help") && stdout.contains("--version") && stdout.contains("USAGE"),
        "--help must describe the supported options, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("cs:") && !stdout.contains("no run modes yet"),
        "--help must not be the failure diagnostic, got: {stdout:?}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "a successful --help must not report a failure on stderr, got: {stderr:?}"
    );

    let created: Vec<_> = fs::read_dir(&workdir)
        .expect("the scratch dir must stay readable")
        .collect();
    assert!(
        created.is_empty(),
        "--help must not run asset discovery or write anything; created: {created:?}"
    );
}

/// AC02 counterpart: `--version` is the same contract — exit 0, no install.
#[test]
fn accept_f00_b_version_without_cs_game_dir_exits_zero() {
    let output = cs()
        .arg("--version")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--version must exit zero with CS_GAME_DIR unset, got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        format!("cs {}", env!("CARGO_PKG_VERSION")),
        "--version must print the workspace version on stdout"
    );
    assert!(
        output.stderr.is_empty(),
        "--version must not report a failure on stderr"
    );
}

/// `--help` must succeed even when the configured installation cannot exist:
/// an implementation that discovered assets first would fail here instead of
/// printing usage.
#[test]
fn accept_f00_b_help_ignores_a_broken_installation_path() {
    let workdir = empty_scratch_dir("accept_f00_b_broken_install_cwd");
    let missing_install = workdir.join("no-such-installation");

    let output = cs()
        .arg("--help")
        .env("CS_GAME_DIR", &missing_install)
        .current_dir(&workdir)
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--help must not depend on the installation, got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "a missing installation must not surface while printing usage"
    );
    assert!(
        fs::read_dir(&workdir)
            .expect("the scratch dir must stay readable")
            .next()
            .is_none(),
        "no asset discovery may happen while printing usage"
    );
}

/// Failure case of the same contract: arguments that name no run mode must
/// still exit 2 with a `cs:` diagnostic. Without this, an implementation that
/// always exits zero would satisfy the `--help` tests.
#[test]
fn accept_f00_b_unsupported_arguments_still_fail_with_diagnostic() {
    let output = cs()
        .args(["--definitely-not-a-run-mode"])
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert!(
        !output.status.success(),
        "unsupported input must not exit zero, got {:?}",
        output.status.code()
    );
    assert_eq!(
        output.status.code(),
        Some(i32::from(cli::EXIT_INVALID_INPUT)),
        "invalid input must use the contract's exit code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cs:") && stderr.contains("--definitely-not-a-run-mode"),
        "the failure must be reported on stderr by name, got: {stderr:?}"
    );
}

/// The production parser itself: flags are classified without reading the
/// environment, unknown input stays a failure.
#[test]
fn accept_f00_b_cli_parse_classifies_flags_without_touching_the_environment() {
    let help = vec!["--help".to_string()];
    assert_eq!(cli::parse(help), CliRequest::Help);

    let short_help = ["-h".to_string()];
    assert_eq!(cli::parse(short_help), CliRequest::Help);

    let version = ["--version".to_string()];
    assert_eq!(cli::parse(version), CliRequest::Version);

    let short_version = ["-V".to_string()];
    assert_eq!(cli::parse(short_version), CliRequest::Version);

    assert_eq!(cli::parse(Vec::<String>::new()), CliRequest::MissingInput);

    let unknown = vec!["--synthetic".to_string(), "--ticks".to_string()];
    assert_eq!(
        cli::parse(unknown.clone()),
        CliRequest::Unsupported { args: unknown }
    );
}
