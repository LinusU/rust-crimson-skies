//! Guard for the owner-maintained CI workflow (F00-C).
//!
//! `.github/workflows/ci.yml` is owner-maintained and protected: an agent
//! that needs a CI change describes it in the review summary or blocks the
//! task instead of editing `.github/`. This module is the local, testable
//! half: it checks that the workflow still runs the three gates the F00
//! feature sheet requires (fmt, clippy with `-D warnings`, the workspace test
//! suite, possibly split across jobs), so an edit that silently drops a gate fails an `accept_f00_c_*`
//! test instead of surfacing later as a green run with no checks.
//!
//! Per-task positive test discovery is deliberately *not* one of these gates:
//! CI runs the whole suite once, and the task-specific prefix selection of
//! [`crate::test_select`] is run locally by the implementing and reviewing
//! agents (`docs/contracts/CLI-EVIDENCE.md`).

use std::fmt;
use std::path::Path;

use crate::bootstrap;
use crate::transient;

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
    // Not matched literally: `verify_workflow` checks that the `cargo test`
    // commands together cover every workspace member. The literal is what a
    // single-job workflow runs and what fixtures are built from.
    (
        "cargo test --workspace --locked",
        &["cargo test --workspace --locked"],
    ),
];

/// Name of the gate that covers the workspace test suite.
const TEST_GATE: &str = REQUIRED_GATES[2].0;

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
    /// A `cargo test` command in the workflow lacks `--locked`.
    TestNotLocked { file: String, command: String },
    /// No `cargo test` command in the workflow tests this workspace member.
    UntestedMember { file: String, member: String },
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
            Self::TestNotLocked { file, command } => {
                write!(f, "{file} runs {command:?} without --locked")
            }
            Self::UntestedMember { file, member } => write!(
                f,
                "{file} never tests workspace member {member}: no cargo test command selects it"
            ),
        }
    }
}

impl std::error::Error for CiError {}

/// Reads the workflow at `path` and verifies every required gate against
/// the workspace members `members` (package names).
pub fn verify_workflow_file(path: &Path, members: &[String]) -> Result<(), CiError> {
    let text = transient::read_to_string(path, transient::PATIENT).map_err(|_| CiError::Io {
        path: path.display().to_string(),
    })?;
    verify_workflow(&path.display().to_string(), &text, members)
}

/// Verifies every required gate in `workflow`'s text.
///
/// fmt and clippy are literal checks. The test gate holds when there is at
/// least one `cargo test` command, every one has `--locked`, and together
/// their selections (`--workspace` minus `--exclude`, or `-p`) cover every
/// entry of `members`. Every failure names what is gone, so a dropped check
/// is reported instead of quietly passing.
pub fn verify_workflow(file: &str, workflow: &str, members: &[String]) -> Result<(), CiError> {
    for (gate, required) in &REQUIRED_GATES[..2] {
        for needle in *required {
            if !workflow.contains(*needle) {
                return Err(CiError::MissingGate {
                    file: file.to_string(),
                    gate,
                    missing: (*needle).to_string(),
                });
            }
        }
    }

    let commands = test_commands(workflow);
    if commands.is_empty() {
        return Err(CiError::MissingGate {
            file: file.to_string(),
            gate: TEST_GATE,
            missing: "cargo test".to_string(),
        });
    }
    for command in &commands {
        if !command.locked {
            return Err(CiError::TestNotLocked {
                file: file.to_string(),
                command: command.text.clone(),
            });
        }
    }
    for member in members {
        if !commands.iter().any(|command| command.tests(member)) {
            return Err(CiError::UntestedMember {
                file: file.to_string(),
                member: member.clone(),
            });
        }
    }
    Ok(())
}

/// Verifies `<workspace_root>/.github/workflows/ci.yml` against the members
/// of the workspace manifest at `<workspace_root>/Cargo.toml`.
pub fn verify_workspace_workflow(workspace_root: &Path) -> Result<(), CiError> {
    let workflow = workspace_root.join(WORKFLOW_PATH);
    // Read the workflow first so a missing workflow is reported as such.
    transient::read_to_string(&workflow, transient::PATIENT).map_err(|_| CiError::Io {
        path: workflow.display().to_string(),
    })?;
    let manifest_path = workspace_root.join("Cargo.toml");
    let io = |path: &Path| CiError::Io {
        path: path.display().to_string(),
    };
    let manifest = transient::read_to_string(&manifest_path, transient::PATIENT)
        .map_err(|_| io(&manifest_path))?;
    let directories = bootstrap::workspace_members(&manifest).map_err(|_| io(&manifest_path))?;
    let members: Vec<String> = directories
        .iter()
        .map(|directory| {
            let member_manifest = workspace_root.join(directory).join("Cargo.toml");
            transient::read_to_string(&member_manifest, transient::PATIENT)
                .ok()
                .and_then(|text| package_name(&text))
                .unwrap_or_else(|| {
                    directory
                        .rsplit('/')
                        .next()
                        .unwrap_or(directory)
                        .to_string()
                })
        })
        .collect();
    verify_workflow_file(&workflow, &members)
}

/// The `name` in the `[package]` table of a member manifest.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(rest) = line.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

/// One `cargo test` invocation found in the workflow.
struct TestCommand {
    text: String,
    locked: bool,
    workspace: bool,
    excluded: Vec<String>,
    packages: Vec<String>,
}

impl TestCommand {
    fn tests(&self, member: &str) -> bool {
        if self.workspace {
            !self.excluded.iter().any(|excluded| excluded == member)
        } else {
            self.packages.iter().any(|package| package == member)
        }
    }
}

/// Every `cargo test` command in `workflow`, outside YAML comments.
/// Arguments after a bare `--` belong to the test harness and are ignored.
fn test_commands(workflow: &str) -> Vec<TestCommand> {
    let mut commands = Vec::new();
    for line in workflow.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(start) = line.find("cargo test") else {
            continue;
        };
        let prefix = line[..start].trim_end();
        let is_command = prefix.is_empty()
            || ["run:", "&&", ";", "|"]
                .iter()
                .any(|lead| prefix.ends_with(lead));
        if !is_command {
            continue;
        }
        let text = line[start..]
            .trim_end_matches(['"', '\''])
            .trim()
            .to_string();
        let mut command = TestCommand {
            text: text.clone(),
            locked: false,
            workspace: false,
            excluded: Vec::new(),
            packages: Vec::new(),
        };
        let mut tokens = text.split_whitespace().skip(2);
        while let Some(token) = tokens.next() {
            match token {
                "--" => break,
                "--locked" => command.locked = true,
                "--workspace" | "--all" => command.workspace = true,
                "--exclude" => command.excluded.extend(tokens.next().map(str::to_string)),
                "-p" | "--package" => command.packages.extend(tokens.next().map(str::to_string)),
                _ => {
                    if let Some(name) = token.strip_prefix("--exclude=") {
                        command.excluded.push(name.to_string());
                    } else if let Some(name) = token.strip_prefix("--package=") {
                        command.packages.push(name.to_string());
                    }
                }
            }
        }
        commands.push(command);
    }
    commands
}
