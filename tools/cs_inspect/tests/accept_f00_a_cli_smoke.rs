//! Command-line smoke for the `cs-inspect` binary (F00-A workspace bootstrap).
//!
//! Same contract as `cs`: invalid or missing input exits nonzero with a
//! diagnostic that names the binary, never zero.

use std::process::Command;

#[test]
fn accept_f00_a_cs_inspect_cli_without_command_fails_with_diagnostic() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .output()
        .expect("the cs-inspect binary must run without a GPU or retail installation");

    assert!(
        !output.status.success(),
        "a missing command must not exit zero, got status {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cs-inspect:"),
        "the failure must be reported on stderr by name, got: {stderr:?}"
    );
}
