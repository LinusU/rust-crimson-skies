//! F00-B / AC02 for `cs-inspect`: `--help` and `--version` exit 0 without a
//! GPU or retail installation and start no asset discovery, while missing or
//! unsupported input keeps failing with a `cs-inspect:` diagnostic.

use std::process::Command;

#[test]
fn accept_f00_b_cs_inspect_help_exits_zero_without_installation() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("--help")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs-inspect binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--help must exit zero with CS_GAME_DIR unset, got {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cs-inspect") && stdout.contains("--version"),
        "--help must describe the binary, got: {stdout:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "a successful --help must not report a failure on stderr"
    );
}

#[test]
fn accept_f00_b_cs_inspect_version_exits_zero_without_installation() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("--version")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs-inspect binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--version must exit zero with CS_GAME_DIR unset"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("cs-inspect {}", env!("CARGO_PKG_VERSION")),
        "--version must print the workspace version on stdout"
    );
}

/// Failure case: an unknown command is still a failure, so a binary that
/// always exits zero cannot pass the help tests.
#[test]
fn accept_f00_b_cs_inspect_unknown_command_still_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("not-a-command")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs-inspect binary must run without a GPU or retail installation");

    assert!(
        !output.status.success(),
        "an unknown command must not exit zero, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cs-inspect:") && stderr.contains("not-a-command"),
        "the failure must be reported on stderr by name, got: {stderr:?}"
    );
}
