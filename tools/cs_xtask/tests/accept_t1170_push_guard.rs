//! Task #1170 (`HOST-PUSH-DEFAULT-GUARD`): a task-branch push must never be
//! able to land on `refs/heads/main` through a stale upstream merge ref.
//!
//! On 2026-10-09 a `git push -u origin <task-branch>` from this shared host
//! pushed the task branch's commits straight to `refs/heads/main`. The
//! mechanism: a branch taken from `origin/main` without `--no-track` records
//! `branch.<name>.merge = refs/heads/main`, `git checkout --no-track -B`
//! keeps that record, and `push.default = upstream` rewrites the push
//! destination from it. The tests below pin the guard
//! (`cs_xtask::push_guard`) that turns that silent rewrite into a loud
//! refusal *before* the push, and — the first test — reproduce the defect
//! with the host's own git so the guard is measured against real git
//! behavior rather than a memory of it. Removing the guard, or letting it
//! resolve the stale merge ref to anything but a refusal, fails them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::push_guard::{self, PushState};
use cs_xtask::transient;

/// The workspace root of this checkout, from the manifest that built the
/// test binary (the fixtures below are hermetic git repositories, so unlike
/// the target-dir gates there is nothing to judge about *this* tree).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Root every fixture repository is created under (inside the gitignored
/// `target/`, so nothing lands in Git). Keyed by process id so two
/// overlapping `cargo test` runs cannot delete each other's trees
/// (task #610's rule for fixture suites).
fn fixtures_root() -> PathBuf {
    workspace_root().join(format!(
        "target/t1170-push-guard-fixtures/{}",
        std::process::id()
    ))
}

/// Runs `git` inside `dir` with global and system git configuration
/// removed, so a host-level `push.default` (the shared host has had one)
/// can neither create nor mask the trap under test. Returns stdout,
/// panicking on any non-zero exit unless `allow_failure`.
fn git(dir: &Path, args: &[&str]) -> String {
    git_raw(dir, args).stdout
}

struct GitOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn git_raw(dir: &Path, args: &[&str]) -> GitOutput {
    let output = transient::command_output(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("HOME", fixtures_root().join("home"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"),
    )
    .expect("git must launch");
    GitOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let output = git_raw(dir, args);
    assert_eq!(
        output.status,
        Some(0),
        "git {args:?} in {} must succeed: {}",
        dir.display(),
        output.stderr
    );
    output.stdout
}

/// A bare origin plus a work repo whose `main` is pushed to it, shaped the
/// way every Rally task branch starts: `git checkout -qb <branch>
/// origin/main`, which records `branch.<branch>.merge = refs/heads/main`.
struct Fixture {
    /// The work repository (the workspace root to hand the guard).
    work: PathBuf,
    /// The bare origin.
    origin: PathBuf,
    /// The task branch checked out in `work`.
    branch: String,
}

fn fixture(name: &str) -> Fixture {
    let root = fixtures_root().join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home")).expect("the fixture home must be creatable");
    let origin = root.join("origin.git");
    let work = root.join("work");
    fs::create_dir_all(&work).expect("the fixture work dir must be creatable");

    git_ok(
        origin.parent().expect("origin has a parent"),
        &["init", "--bare", "-q", "origin.git"],
    );
    git_ok(&work, &["init", "-q"]);
    git_ok(&work, &["config", "user.email", "fixture@example.invalid"]);
    git_ok(&work, &["config", "user.name", "t1170-fixture"]);
    git_ok(&work, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fs::write(work.join("seed.txt"), "seed\n").expect("the seed file must be writable");
    git_ok(&work, &["add", "seed.txt"]);
    git_ok(&work, &["commit", "-qm", "seed"]);
    git_ok(
        &work,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("utf-8 path"),
        ],
    );
    git_ok(&work, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git_ok(&work, &["fetch", "-q", "origin"]);

    let branch = "rally/1170-fixture".to_string();
    git_ok(&work, &["checkout", "-qb", &branch, "origin/main"]);
    // The incident's precondition, asserted so the fixture cannot silently
    // stop reproducing it when git's automatic upstream setup changes:
    assert_eq!(
        git(
            &work,
            &["config", "--get", &format!("branch.{branch}.merge")]
        ),
        "refs/heads/main",
        "checking out from origin/main must record the stale merge ref"
    );
    fs::write(work.join("change.txt"), "change\n").expect("the change file must be writable");
    git_ok(&work, &["add", "change.txt"]);
    git_ok(&work, &["commit", "-qm", "change"]);
    Fixture {
        work,
        origin,
        branch,
    }
}

/// Runs `cs_xtask push-guard` in `work` with the same isolated git
/// configuration as the fixtures.
fn push_guard(work: &Path, extra: &[&str]) -> GitOutput {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let output = transient::command_output(
        Command::new(bin)
            .arg("push-guard")
            .arg("--workspace-root")
            .arg(work)
            .args(extra)
            .env("HOME", fixtures_root().join("home"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"),
    )
    .expect("the cs_xtask binary must run");
    GitOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    }
}

/// The defect, reproduced with the host's own git: under
/// `push.default = upstream` and the stale `branch.<name>.merge =
/// refs/heads/main`, the very command the agent instructions give —
/// `git push -u origin <task-branch>` — updates `refs/heads/main` on the
/// origin. This test pins the *mechanism* the guard defends against, so a
/// future git that stops rewriting the destination is visible here rather
/// than silently making the guard's model wrong.
#[test]
fn accept_t1170_the_unguarded_push_really_updates_main() {
    let fixture = fixture("reproduction");
    git_ok(&fixture.work, &["config", "push.default", "upstream"]);
    let head_before = git_ok(&fixture.origin, &["rev-parse", "refs/heads/main"]);

    let output = git_raw(&fixture.work, &["push", "-u", "origin", &fixture.branch]);
    assert_eq!(
        output.status,
        Some(0),
        "the reproduction push itself must succeed: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("-> main"),
        "the measured incident was the task branch updating main, got: {}",
        output.stderr
    );
    let task_head = git_ok(&fixture.work, &["rev-parse", "HEAD"]);
    let head_after = git_ok(&fixture.origin, &["rev-parse", "refs/heads/main"]);
    assert_ne!(head_before, head_after);
    assert_eq!(
        head_after, task_head,
        "the unguarded push must move the origin's main to the task branch's commit"
    );
}

/// The guard against exactly that setup: `push-guard` refuses, names
/// `refs/heads/main` as the destination a push would update, and spells out
/// the fixes — and the agent following it never runs the push, so the
/// origin's main stays where it was.
#[test]
fn accept_t1170_push_guard_refuses_the_trap_before_anything_is_pushed() {
    let fixture = fixture("guard-refuses");
    git_ok(&fixture.work, &["config", "push.default", "upstream"]);
    let main_before = git_ok(&fixture.origin, &["rev-parse", "refs/heads/main"]);

    let report = push_guard(&fixture.work, &[]);
    assert_eq!(
        report.status,
        Some(1),
        "push-guard must refuse the stale-merge-ref push, stdout: {}, stderr: {}",
        report.stdout,
        report.stderr
    );
    assert!(
        report
            .stdout
            .contains("a push would update: refs/heads/main"),
        "the refusal must print the exact destination a push would update, got: {}",
        report.stdout
    );
    for expected in [
        "refs/heads/main",
        "push.default",
        "push.default=upstream",
        "branch.rally/1170-fixture.merge",
        "HEAD:refs/heads/",
    ] {
        assert!(
            report.stderr.contains(expected),
            "the refusal must mention {expected:?}, got: {}",
            report.stderr
        );
    }
    // Nothing was pushed: the origin's main still holds its old commit.
    assert_eq!(
        git_ok(&fixture.origin, &["rev-parse", "refs/heads/main"]),
        main_before,
        "refusing the push must leave the origin untouched"
    );
}

/// The acceptance criterion, positive side: with `push.default = current`
/// — the fix the incident points at — the same stale merge ref is inert:
/// the guard resolves the push to the task branch itself and allows it.
#[test]
fn accept_t1170_push_guard_allows_the_same_branch_under_push_default_current() {
    let fixture = fixture("guard-allows");
    git_ok(&fixture.work, &["config", "push.default", "current"]);

    let report = push_guard(&fixture.work, &[]);
    assert_eq!(
        report.status,
        Some(0),
        "push.default=current must pass the guard even with a stale merge ref, stderr: {}",
        report.stderr
    );
    assert!(
        report
            .stdout
            .contains("a push would update: refs/heads/rally/1170-fixture"),
        "the guard must print the resolved task-branch destination, got: {}",
        report.stdout
    );
    assert!(
        !report.stdout.contains("would update: refs/heads/main"),
        "the guard must not name main as a destination under push.default=current"
    );
}

/// The other measured safe forms and the CLI's own edges: `simple` (git's
/// default) resolves to the task branch; `--branch` judges a named branch;
/// a detached HEAD, an unknown `push.default` and a non-workspace root are
/// loud failures; a bad option is a usage error.
#[test]
fn accept_t1170_push_guard_cli_edges_are_loud() {
    // `simple` — git's default when push.default is unset — resolves to the
    // branch itself even with the stale merge ref present (measured on the
    // host's git in the same shape).
    let fixture = fixture("cli-simple");
    let report = push_guard(&fixture.work, &[]);
    assert_eq!(
        report.status,
        Some(0),
        "the default push.default=simple must pass, stderr: {}",
        report.stderr
    );
    assert!(
        report.stdout.contains("push.default=simple")
            && report
                .stdout
                .contains("a push would update: refs/heads/rally/1170-fixture"),
        "the guard must report git's default and its destination, got: {}",
        report.stdout
    );

    // --branch judges a named branch instead of the checked-out one.
    let named = push_guard(&fixture.work, &["--branch", "rally/1170-fixture"]);
    assert_eq!(
        named.status,
        Some(0),
        "--branch with the task branch must pass, stderr: {}",
        named.stderr
    );
    // A branch this repository does not hold resolves through the same
    // rules; the guard notes that git itself would refuse the src refspec
    // rather than pretending the push is possible.
    let foreign = push_guard(&fixture.work, &["--branch", "rally/no-such-upstream"]);
    assert_eq!(
        foreign.status,
        Some(0),
        "an unknown branch under push.default=simple has no upstream, so it resolves to \
         its own name; stderr: {}",
        foreign.stderr
    );
    assert!(
        foreign.stdout.contains("not a local branch"),
        "the guard must note that the named branch does not exist locally, got: {}",
        foreign.stdout
    );

    // A detached HEAD cannot resolve a push at all.
    git_ok(&fixture.work, &["checkout", "--detach", "HEAD"]);
    let detached = push_guard(&fixture.work, &[]);
    assert_eq!(
        detached.status,
        Some(1),
        "a detached HEAD must fail the guard, stdout: {}",
        detached.stdout
    );
    assert!(
        detached.stderr.contains("detached"),
        "the failure must say HEAD is detached, got: {}",
        detached.stderr
    );

    // A root git cannot resolve fails loudly (exit 1) rather than passing.
    let missing_root = fixtures_root().join("cli-simple/no-such-root");
    let unusable = push_guard(&missing_root, &[]);
    assert_eq!(
        unusable.status,
        Some(1),
        "a root git cannot resolve must fail the gate"
    );
    assert!(
        unusable.stderr.contains("could not") || unusable.stderr.contains("fatal"),
        "the failure must carry git's own message, got: {}",
        unusable.stderr
    );

    // Unknown or incomplete options are usage errors, never silent ignores.
    let bad_option = push_guard(&fixture.work, &["--bogus"]);
    assert_eq!(
        bad_option.status,
        Some(2),
        "an unknown option must be usage"
    );
    let missing_value = {
        let bin = env!("CARGO_BIN_EXE_cs_xtask");
        transient::command_output(
            Command::new(bin)
                .args(["push-guard", "--branch"])
                .env("HOME", fixtures_root().join("home"))
                .env("GIT_CONFIG_GLOBAL", "/dev/null"),
        )
        .expect("the cs_xtask binary must run")
    };
    assert_eq!(
        missing_value.status.code(),
        Some(2),
        "--branch without a value must be usage"
    );
}

/// The resolution rules themselves, against synthetic states: the four
/// `push.default` values the incident and the fixes were measured under,
/// plus the values that must refuse rather than guess. These call
/// production code directly and need no git at all.
#[test]
fn accept_t1170_resolve_rules_match_the_measured_git_behavior() {
    let branch = "rally/1170-x".to_string();
    let state = |push_default: &str, merge: Option<&str>| PushState {
        branch: Some(branch.clone()),
        push_default: Some(push_default.to_string()),
        upstream_remote: Some("origin".to_string()),
        upstream_merge: merge.map(str::to_owned),
        local_branches: vec!["main".to_string(), branch.clone()],
    };

    // The trap: upstream + stale merge ref resolves to main and refuses.
    let report = push_guard::resolve(&state("upstream", Some("refs/heads/main")));
    assert_eq!(report.destinations, vec!["refs/heads/main"]);
    assert!(!report.is_allowed(), "the trap must be refused");

    // tracking is the legacy spelling of upstream — same rewrite.
    let report = push_guard::resolve(&state("tracking", Some("refs/heads/main")));
    assert!(!report.is_allowed());

    // The shorthand spelling `merge = main` is caught too.
    let report = push_guard::resolve(&state("upstream", Some("main")));
    assert_eq!(report.destinations, vec!["refs/heads/main"]);
    assert!(!report.is_allowed());

    // An upstream pointing at a non-protected branch is allowed.
    let report = push_guard::resolve(&state("upstream", Some("refs/heads/other")));
    assert_eq!(report.destinations, vec!["refs/heads/other"]);
    assert!(report.is_allowed());

    // current ignores the stale merge ref (the fix).
    let report = push_guard::resolve(&state("current", Some("refs/heads/main")));
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());

    // simple with a matching or absent upstream resolves to the branch.
    let report = push_guard::resolve(&state("simple", Some("refs/heads/rally/1170-x")));
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());
    let report = push_guard::resolve(&state("simple", None));
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());
    // simple with a *different* upstream (the stale main): the destination
    // stays the task branch — measured — but the note says a bare
    // `git push` would be refused by git, so the broken config is not
    // silently tolerated.
    let report = push_guard::resolve(&state("simple", Some("refs/heads/main")));
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.contains("bare `git push`")),
        "the stale upstream under simple must be noted, got: {:?}",
        report.notes
    );

    // upstream with no upstream configured: git refuses, so does the guard.
    let report = push_guard::resolve(&state("upstream", None));
    assert!(!report.is_allowed());

    // matching lists every local branch as a candidate — and refuses when a
    // candidate is protected, since the remote side is unknowable offline.
    let report = push_guard::resolve(&state("matching", None));
    assert_eq!(
        report.destinations,
        vec!["refs/heads/main", "refs/heads/rally/1170-x"]
    );
    assert!(!report.is_allowed());
    let mut no_main = state("matching", None);
    no_main.local_branches = vec![branch.clone()];
    let report = push_guard::resolve(&no_main);
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());

    // nothing pushes nothing without an explicit refspec.
    let report = push_guard::resolve(&state("nothing", Some("refs/heads/main")));
    assert!(report.destinations.is_empty());
    assert!(report.is_allowed());

    // An unmeasured push.default refuses rather than guessing.
    let report = push_guard::resolve(&state("banana", None));
    assert!(!report.is_allowed());

    // A detached HEAD refuses rather than resolving nothing.
    let report = push_guard::resolve(&PushState {
        branch: None,
        ..state("current", None)
    });
    assert!(!report.is_allowed());
}

/// The default when no `push.default` is configured anywhere is git's
/// `simple`, not an unbounded push: with a stale merge ref the resolution
/// still lands on the task branch.
#[test]
fn accept_t1170_unset_push_default_resolves_as_simple() {
    let report = push_guard::resolve(&PushState {
        branch: Some("rally/1170-x".to_string()),
        push_default: None,
        upstream_remote: None,
        upstream_merge: Some("refs/heads/main".to_string()),
        local_branches: Vec::new(),
    });
    assert_eq!(report.push_default, "simple");
    assert_eq!(report.destinations, vec!["refs/heads/rally/1170-x"]);
    assert!(report.is_allowed());
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.contains("bare `git push`")),
        "the stale upstream must still be surfaced as a note, got: {:?}",
        report.notes
    );
}
