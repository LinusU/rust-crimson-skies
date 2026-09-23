//! Guard for the owner-maintained CI workflow (F00-C).
//!
//! `.github/workflows/ci.yml` is owner-maintained and protected: an agent
//! that needs a CI change describes it in the review summary or blocks the
//! task instead of editing `.github/`. This module is the local, testable
//! half: it checks that the workflow still runs the three gates the F00
//! feature sheet requires (fmt, clippy with `-D warnings`, the workspace test
//! suite), so an edit that silently drops a gate fails an `accept_f00_c_*`
//! test instead of surfacing later as a green run with no checks.
//!
//! Per-task positive test discovery is deliberately *not* one of these gates:
//! CI runs the whole suite once, and the task-specific prefix selection of
//! [`crate::test_select`] is run locally by the implementing and reviewing
//! agents (`docs/contracts/CLI-EVIDENCE.md`).

use std::fmt;
use std::fs;
use std::path::Path;

/// Workflow file the workspace gates live in, relative to the workspace root.
pub const WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// A gate CI must run: a stable name plus every literal the workflow has to
/// keep for the gate to be real.
pub const REQUIRED_GATES: [(&str, &[&str]); 3] = [
    (
        "cargo fmt --all -- --check",
        &["cargo fmt --all -- --check"],
    ),
    (
        "cargo clippy with -D warnings",
        &[
            "cargo clippy --workspace --all-targets --all-features --locked",
            "-D warnings",
        ],
    ),
    (
        "cargo test --workspace --locked",
        &["cargo test --workspace --locked"],
    ),
];

/// Why the workflow cannot be trusted to gate the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CiError {
    /// The workflow file could not be read.
    Io { path: String },
    /// A required gate is missing from the workflow.
    MissingGate {
        file: String,
        gate: &'static str,
        missing: String,
    },
}

impl fmt::Display for CiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::MissingGate {
                file,
                gate,
                missing,
            } => write!(
                f,
                "{file} does not run the {gate} gate: it never mentions {missing:?}"
            ),
        }
    }
}

impl std::error::Error for CiError {}

/// Reads the workflow at `path` and verifies every required gate.
pub fn verify_workflow_file(path: &Path) -> Result<(), CiError> {
    let text = fs::read_to_string(path).map_err(|_| CiError::Io {
        path: path.display().to_string(),
    })?;
    verify_workflow(&path.display().to_string(), &text)
}

/// Verifies every required gate in `workflow`'s text.
///
/// Every failure names the gate and the literal that is gone, so a dropped
/// check is reported instead of quietly passing.
pub fn verify_workflow(file: &str, workflow: &str) -> Result<(), CiError> {
    for (gate, required) in REQUIRED_GATES {
        for needle in required {
            if !workflow.contains(*needle) {
                return Err(CiError::MissingGate {
                    file: file.to_string(),
                    gate,
                    missing: (*needle).to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Verifies `<workspace_root>/.github/workflows/ci.yml`.
pub fn verify_workspace_workflow(workspace_root: &Path) -> Result<(), CiError> {
    verify_workflow_file(&workspace_root.join(WORKFLOW_PATH))
}
