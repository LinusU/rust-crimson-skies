//! Command-line smoke for the `cs` binary (F00-A workspace bootstrap).
//!
//! The binary must never report failure as success: with no usable input it
//! exits nonzero with a diagnostic that names the binary and the missing
//! input (CLI-EVIDENCE contract: "Never return zero after only logging a
//! failure").

use std::process::Command;

#[test]
fn accept_f00_a_cs_cli_without_input_fails_with_diagnostic() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert!(
        !output.status.success(),
        "no input must not exit zero, got status {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cs:"),
        "the failure must be reported on stderr by name, got: {stderr:?}"
    );
}
