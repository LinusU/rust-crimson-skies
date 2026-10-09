//! Guard a task-branch push against a stale upstream merge ref (task #1170).
//!
//! On 2026-10-09 (task #1155) a `git push -u origin <task-branch>` from this
//! shared host pushed the task branch's commits straight to `refs/heads/main`
//! on the remote. The push was repaired within minutes, but the trap is open
//! for every agent until a guard stands in front of it. The mechanism, all
//! reproduced in a scratch repository on the host's git (2.50.1) before this
//! module was written:
//!
//! 1. A branch taken from `origin/main` without `--no-track` records
//!    `branch.<name>.remote = origin` and `branch.<name>.merge =
//!    refs/heads/main` (git's automatic upstream setup).
//! 2. A later `git checkout --no-track -B <name> …` does **not** remove the
//!    pre-existing `branch.<name>.merge`; `--no-track` only skips the setup
//!    of a *new* upstream, it never tears one down.
//! 3. With `push.default = upstream` — the repo-wide value on this host when
//!    the incident happened — git *rewrites the destination* of the
//!    command-line refspec from that stale merge ref. Measured, both forms
//!    land on main:
//!
//!    ```text
//!    $ git config --get push.default
//!    upstream
//!    $ git config --get branch.rally/1170-test.merge
//!    refs/heads/main
//!    $ git push -u origin rally/1170-test
//!     * [new branch]      rally/1170-test -> main        <-- the incident
//!    $ git push -u origin
//!       6a5cf9b..2a21f5f  f5-nobrancharg -> main         <-- same trap
//!    ```
//!
//!    `push.default = current` pushes the same setup to
//!    `refs/heads/rally/1170-test`, `push.default = simple` does too, and an
//!    explicit destination (`git push -u origin HEAD:refs/heads/<name>`)
//!    always wins — all three were measured in the same scratch repo.
//!
//! [`check`] answers the only question that matters before an agent pushes:
//! **which remote refs would a push from this branch update**, and it refuses
//! the push when any of them is the protected `refs/heads/main`. The
//! resolution mirrors git's own for the forms agents use (bare `git push`
//! and `git push [-u] [remote] <branch>`), reading every fact from git
//! itself — `symbolic-ref`, `config --get`, `for-each-ref` — so no git
//! precedence rule is reimplemented from memory. The one thing it never does
//! is talk to the remote: "does the remote already have this branch" is a
//! question this guard does not need to answer to refuse a push at main.
//!
//! `push.default = matching` is the one value whose destinations cannot be
//! resolved offline (they are the branches that exist on *both* sides), so
//! the guard treats every local branch as a candidate destination and
//! refuses when a candidate is protected — a superset, never a guess that
//! something is safe.

use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::transient;

/// The one ref no agent push may ever update: only Rally's landing flow
/// moves main, and only onto a reviewed commit with green CI.
pub const PROTECTED_REF: &str = "refs/heads/main";

/// Why the guard itself could not run.
#[derive(Debug)]
pub enum PushGuardError {
    /// `git` could not be started at all.
    Launch { program: String, source: io::Error },
    /// `git` ran but failed for a reason other than "the answer is absent".
    Git {
        arguments: String,
        status: String,
        tail: String,
    },
}

impl fmt::Display for PushGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Launch { program, source } => {
                write!(f, "{program} could not start: {source}")
            }
            Self::Git {
                arguments,
                status,
                tail,
            } => write!(f, "{arguments} exited {status}: {tail}"),
        }
    }
}

impl std::error::Error for PushGuardError {}

/// Every fact about the branch a push would start from, read from git.
///
/// Every field comes from `git` itself, so the guard never parses
/// `.git/config` or re-implements config precedence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushState {
    /// The checked-out branch ([`None`] on a detached HEAD).
    pub branch: Option<String>,
    /// `push.default` as configured ([`None`] when unset: git's default is
    /// `simple`).
    pub push_default: Option<String>,
    /// `branch.<name>.remote` for the branch ([`None`] when unset).
    pub upstream_remote: Option<String>,
    /// `branch.<name>.merge` for the branch — the stale
    /// `refs/heads/main` of the incident ([`None`] when unset).
    pub upstream_merge: Option<String>,
    /// Every local branch, for `push.default = matching`.
    pub local_branches: Vec<String>,
}

/// What a push from this branch would update, and why it may not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The branch the resolution was made for.
    pub branch: Option<String>,
    /// The effective `push.default` (git's `simple` when unset).
    pub push_default: String,
    /// The remote a `git push` would go to (`branch.<name>.remote`, else
    /// git's `origin` default).
    pub remote: String,
    /// The remote refs a push would update, in push order. Empty when git
    /// itself would push nothing (or refuse).
    pub destinations: Vec<String>,
    /// Things a caller should know that do not refuse the push.
    pub notes: Vec<String>,
    /// Why the push must not run. Empty exactly when [`Report::is_allowed`].
    pub refusals: Vec<String>,
}

impl Report {
    /// Whether the push may go ahead: no refusal, and no destination is the
    /// protected ref.
    pub fn is_allowed(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// Reads [`PushState`] for `repo` from git itself. `branch_override` names
/// the branch to judge instead of the checked-out one (the branch an agent
/// is about to push, which may not be checked out yet).
pub fn read_state(repo: &Path, branch_override: Option<&str>) -> Result<PushState, PushGuardError> {
    let branch = match branch_override {
        Some(name) => Some(name.to_owned()),
        // `symbolic-ref --quiet` exits 1 silently on a detached HEAD: not
        // an error, the answer is "there is no branch".
        None => git_output(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?,
    };
    let push_default = git_config_get(repo, "push.default")?;
    let (upstream_remote, upstream_merge) = match &branch {
        Some(name) => (
            git_config_get(repo, &format!("branch.{name}.remote"))?,
            git_config_get(repo, &format!("branch.{name}.merge"))?,
        ),
        None => (None, None),
    };
    let local_branches = git_for_each_ref(repo, "refs/heads")?;
    Ok(PushState {
        branch,
        push_default,
        upstream_remote,
        upstream_merge,
        local_branches,
    })
}

/// Resolves the destinations of a push from `state` the way git does for
/// bare `git push` and `git push [-u] [remote] <branch>` (no explicit
/// destination on the command line), and refuses any resolution that would
/// update [`PROTECTED_REF`].
pub fn resolve(state: &PushState) -> Report {
    let push_default = state
        .push_default
        .clone()
        .unwrap_or_else(|| "simple".to_string());
    let remote = state
        .upstream_remote
        .clone()
        .unwrap_or_else(|| "origin".to_string());
    let mut report = Report {
        branch: state.branch.clone(),
        push_default: push_default.clone(),
        remote,
        destinations: Vec::new(),
        notes: Vec::new(),
        refusals: Vec::new(),
    };
    let Some(branch) = state.branch.clone() else {
        report.refusals.push(
            "HEAD is detached, so there is no branch for git to resolve a push from; check \
             out your task branch first"
                .to_string(),
        );
        return report;
    };

    match push_default.as_str() {
        // Measured: `current` ignores the stale upstream entirely and pushes
        // the branch to its own name. This is the fix the incident points at.
        "current" => {
            report.destinations.push(format!("refs/heads/{branch}"));
        }
        // Measured: `upstream`/`tracking` rewrite the destination through
        // branch.<name>.merge — the incident's rewrite to main.
        "upstream" | "tracking" => match &state.upstream_merge {
            Some(merge) => report.destinations.push(normalize_ref(merge)),
            None => report.refusals.push(format!(
                "push.default={push_default} needs branch.{branch}.merge to say where to push, \
                 and it is unset — git itself refuses this push; either re-point the upstream \
                 (git branch --set-upstream-to=origin/{branch}) or push an explicit refspec \
                 (git push -u origin HEAD:refs/heads/{branch})"
            )),
        },
        // Measured: `simple` pushes the branch to its own name. A stale
        // upstream of a *different* name does not rewrite the destination
        // (`git push -u origin f3-simple` pushed `f3-simple -> f3-simple`
        // with `branch.f3-simple.merge = refs/heads/main`); what it does do
        // is make a *bare* `git push` refused by git itself. Both facts go
        // to the caller: the safe destination, and the note that the config
        // is broken for the bare form.
        "simple" => {
            if let Some(merge) = &state.upstream_merge
                && normalize_ref(merge) != format!("refs/heads/{branch}")
            {
                report.notes.push(format!(
                    "push.default=simple and branch.{branch}.merge points at {merge}, a \
                     different branch, so a bare `git push` would be refused by git; the \
                     destination below is the explicit-branch form's"
                ));
            }
            report.destinations.push(format!("refs/heads/{branch}"));
        }
        // Measured: `matching` pushes every branch that exists on both sides.
        // The remote side cannot be resolved offline, so every local branch
        // is a candidate destination — a superset, never a claim that the
        // push is small.
        "matching" => {
            report.notes.push(
                "push.default=matching pushes every branch that exists on both sides; the \
                 remote side is not resolved offline, so every local branch below is a \
                 candidate destination"
                    .to_string(),
            );
            for name in &state.local_branches {
                report.destinations.push(format!("refs/heads/{name}"));
            }
        }
        // Measured: `nothing` pushes nothing unless the command line spells
        // a refspec out.
        "nothing" => {
            report.notes.push(
                "push.default=nothing pushes nothing without an explicit refspec, so no \
                 destination can be resolved from configuration"
                    .to_string(),
            );
        }
        other => {
            report.refusals.push(format!(
                "push.default={other:?} is not one of the values this guard measured \
                 (current, simple, upstream, tracking, matching, nothing); refusing rather \
                 than guessing where a push would land"
            ));
        }
    }

    // A branch the configuration names but this repository does not hold
    // cannot be pushed at all — git itself refuses such a src refspec — so
    // it is worth saying, but it is not a reason to refuse: the destinations
    // below are what the configuration resolves to.
    if !report.destinations.is_empty()
        && !state.local_branches.is_empty()
        && !state.local_branches.iter().any(|name| name == &branch)
    {
        report.notes.push(format!(
            "{branch} is not a local branch in this repository, so git itself would refuse \
             this push; the destination below is what the configuration resolves to"
        ));
    }

    if report.destinations.iter().any(|dest| is_protected(dest)) {
        report.refusals.push(format!(
            "a push from {branch} with push.default={} would update {PROTECTED_REF}: that is \
             Rally's landing flow's ref, never an agent's. Fix the configuration — git \
             config push.default current — and re-point or remove the stale merge ref — git \
             config --unset branch.{branch}.merge — or push an explicit refspec: git push -u \
             origin HEAD:refs/heads/{branch}",
            report.push_default
        ));
    }
    report
}

/// Reads the state and resolves it in one step.
pub fn check(repo: &Path, branch_override: Option<&str>) -> Result<Report, PushGuardError> {
    Ok(resolve(&read_state(repo, branch_override)?))
}

/// Whether a destination ref is the protected one. Both spellings git and
/// the config accept are compared, so `merge = main` is caught too.
fn is_protected(destination: &str) -> bool {
    destination == PROTECTED_REF || destination == "main"
}

/// Full ref for a `branch.<name>.merge` value: git stores full refs, but a
/// hand-written config may hold the shorthand.
fn normalize_ref(value: &str) -> String {
    if value.starts_with("refs/") {
        value.to_owned()
    } else {
        format!("refs/heads/{value}")
    }
}

fn git_command(repo: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo);
    command.args(arguments);
    command
}

/// Runs git and returns its trimmed stdout, or [`None`] when git exited 1
/// with no output — the "absent, not failed" answer of `symbolic-ref
/// --quiet` and `config --get`.
fn git_output(repo: &Path, arguments: &[&str]) -> Result<Option<String>, PushGuardError> {
    let output =
        transient::command_output(&mut git_command(repo, arguments)).map_err(|source| {
            PushGuardError::Launch {
                program: "git".to_string(),
                source,
            }
        })?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let absent = output.status.code() == Some(1) && stderr.is_empty();
    if absent {
        Ok(None)
    } else {
        Err(PushGuardError::Git {
            arguments: format!("git -C {} {}", repo.display(), arguments.join(" ")),
            status: output.status.to_string(),
            tail: stderr,
        })
    }
}

/// `git config --get <key>`: [`None`] when the key is unset (git exits 1
/// silently), an error when git itself failed.
fn git_config_get(repo: &Path, key: &str) -> Result<Option<String>, PushGuardError> {
    git_output(repo, &["config", "--get", key]).map(|value| value.filter(|value| !value.is_empty()))
}

/// `git for-each-ref <pattern>` one short name per line.
fn git_for_each_ref(repo: &Path, pattern: &str) -> Result<Vec<String>, PushGuardError> {
    let Some(output) = git_output(
        repo,
        &["for-each-ref", "--format=%(refname:short)", pattern],
    )?
    else {
        return Ok(Vec::new());
    };
    Ok(output
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}
