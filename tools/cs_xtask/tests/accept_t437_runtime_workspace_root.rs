//! Task #437: a live gate must judge the checkout it is *running* in.
//!
//! The `accept_t383_`, `accept_t433_` and `accept_t440_` suites each run
//! [`target_dir::verify_workspace`] against this very checkout, so
//! `cargo test --workspace` fails an agent whose environment still exports a
//! shared `CARGO_TARGET_DIR`. To judge a checkout the gate first has to *name*
//! it, and for a long time each of those suites named it with
//! `env!("CARGO_MANIFEST_DIR")/../..`.
//!
//! That path is baked into the binary when it is compiled, and a compiled test
//! binary can outlive the tree that built it. Cargo keys artifacts by package id
//! and fingerprint, never by source path, so a `CARGO_TARGET_DIR` shared between
//! checkouts — or one naming a checkout that was later replaced — lets `cargo
//! test` run a foreign artifact here without rebuilding anything. That artifact
//! names the checkout that compiled it, so the gate compares a perfectly private
//! `…/f18b/target` against a baked root of `…/devin-1` and reports:
//!
//! ```text
//! the effective cargo target directory /…/f18b/target is not private to this
//! worktree /…/devin-1
//! ```
//!
//! Nothing in the environment under test is wrong, so the failure has no cause
//! the agent can act on. It also fires on unmodified `main`, which is the tell
//! that the binary, not the checkout, is stale.
//!
//! [`target_dir::running_workspace_root`] fixes it by deriving the root at run
//! time from the process's working directory — cargo runs test binaries with the
//! package root as the cwd, so the walk starts inside the checkout under test —
//! and these tests pin that derivation so it cannot silently go back to a
//! compile-time constant. The negative direction matters just as much: a
//! directory genuinely shared between two live checkouts must still be refused
//! loudly, which the end-to-end test below drives through the real `cargo
//! metadata` path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::target_dir;

/// Root every fixture tree is created under (inside the gitignored `target/`, so
/// nothing lands in Git and `cargo clean` reaps it).
fn fixtures_root() -> PathBuf {
    target_dir::running_workspace_root()
        .expect("the checkout this test runs in must have a workspace manifest")
        .join("target/t437-runtime-workspace-root")
}

/// Root for the fixtures that must have **no** workspace manifest above them.
///
/// These cannot live under [`fixtures_root`]: this checkout's own `Cargo.toml`
/// carries `[workspace]`, so the walk would always reach it and every negative
/// case would pass for the wrong reason. Outside it, no ancestor is a cargo
/// workspace — which is what `accept_t383_`'s unreadable-workspace fixture
/// already relies on for `cargo metadata` to fail.
///
/// `case` names the caller, so the root is unique per test rather than only per
/// process: the tests in one binary run in parallel threads, and a single
/// process-keyed root would let either test's `remove_dir_all` delete the tree
/// the other is writing (task #462). The directory stays under
/// `std::env::temp_dir()` and is removed and recreated on every call, so a
/// stale tree from an interrupted run cannot leak in.
fn outside_root(case: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("t437-no-root-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the fixture root must be creatable");
    root
}

/// Pins the property the fix is about, deterministically rather than by timing:
/// two callers get two roots, so neither test's `remove_dir_all` can delete the
/// tree the other is writing (task #462).
///
/// It uses the dedicated `guard-alpha`/`guard-beta` names, never the ones the
/// two real tests use: sharing a caller name with a running test would make this
/// guard itself the racer.
#[test]
fn accept_t437_outside_roots_are_unique_per_caller() {
    let alpha = outside_root("guard-alpha");
    let beta = outside_root("guard-beta");
    assert_ne!(
        alpha, beta,
        "two callers must not share one fixture root, or whichever calls \
         remove_dir_all first deletes the other test's tree mid-write"
    );
    assert!(
        alpha.is_dir() && beta.is_dir(),
        "both per-caller roots must exist after creation"
    );

    // Re-entering one caller's root resets only that tree; the other caller's
    // stays intact. A single process-keyed root would destroy both.
    let alpha_again = outside_root("guard-alpha");
    assert_eq!(
        alpha, alpha_again,
        "a caller's root is stable for the life of one test process"
    );
    assert!(
        beta.is_dir(),
        "recreating one caller's root must not delete another's"
    );

    let _ = fs::remove_dir_all(&alpha);
    let _ = fs::remove_dir_all(&beta);
}

/// A detached cargo workspace holding one package, written at `root`.
///
/// `layout` names directories below `root` that get a plain `[package]`
/// manifest; `root` itself gets the `[workspace]` one. Two checkouts with
/// different basenames is the fleet layout, so a shared target directory has to
/// be refused on the name alone.
fn checkout(root: &Path, layout: &[&str]) -> PathBuf {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).expect("the fixture root must be creatable");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = []\n",
    )
    .expect("the fixture workspace manifest must be writable");
    for relative in layout {
        let dir = root.join(relative);
        fs::create_dir_all(&dir).expect("the fixture package dir must be creatable");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .expect("the fixture package manifest must be writable");
    }
    root.to_path_buf()
}

/// The defect itself, as a test rather than as a reproduction someone has to
/// The defect itself, as a test rather than as a reproduction someone has to
/// run by hand: `running_workspace_root` reads the *process's* working
/// directory, so it can only be pinned by re-executing this very test binary
/// from a different directory. The parent sets [`RUNNING_ROOT_EXPECTED`] and
/// this test then reports the root it derived; with the variable unset — the
/// ordinary run — it asserts the in-checkout case instead, so both halves run on
/// every `cargo test`.
///
/// The parent half is the part that matters: a binary compiled in *this*
/// checkout, started from a *different* checkout, must name the checkout it is
/// running in. It cannot, if the root comes from `env!("CARGO_MANIFEST_DIR")`.
/// The expectation travels in an environment variable rather than an argument
/// because the test harness would read a bare positional argument as a filter.
#[test]
fn accept_t437_the_running_root_follows_the_working_directory() {
    let root = target_dir::running_workspace_root().expect("a workspace root");

    let Some(expected) = std::env::var_os(RUNNING_ROOT_EXPECTED) else {
        // Ordinary run: this checkout is its own running root.
        assert!(
            root.join("Cargo.toml").is_file(),
            "the derived root must be the checkout running the test, got {}",
            root.display()
        );
        return;
    };

    let expected = PathBuf::from(expected);
    println!("{}", root.display());
    assert_eq!(
        root,
        expected,
        "a binary compiled elsewhere, run from {}, must name that checkout, not \
         the one it was compiled in",
        expected.display()
    );
}

/// The root a re-executed copy of this binary is expected to report, set by
/// [`accept_t437_a_binary_compiled_elsewhere_names_the_checkout_it_runs_in`].
const RUNNING_ROOT_EXPECTED: &str = "CS_T437_EXPECTED_RUNNING_ROOT";

/// Drives the child half of
/// [`accept_t437_the_running_root_follows_the_working_directory`]: this test
/// binary, compiled in this checkout, launched from a fixture workspace that
/// was never built in. It is the reported environment — cargo reusing a foreign
/// artifact — reduced to one process start, so the regression cannot come back
/// unnoticed on a machine where nobody happens to have a stale `target/`.
#[test]
fn accept_t437_a_binary_compiled_elsewhere_names_the_checkout_it_runs_in() {
    let root = checkout(&fixtures_root().join("foreign"), &["tools/probe"]);
    let binary = std::env::current_exe().expect("the test binary has a path");

    let output = Command::new(&binary)
        .current_dir(&root)
        .args([
            "--exact",
            "accept_t437_the_running_root_follows_the_working_directory",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(RUNNING_ROOT_EXPECTED, &root)
        .output()
        .expect("the test binary must re-execute");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "a binary compiled in this checkout, run from {}, must derive that \
         checkout as its running root; stdout: {stdout}\nstderr: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains(&root.display().to_string()),
        "the child must name {} as the checkout it runs in, not the one it was \
         compiled in; stdout: {stdout}",
        root.display(),
    );
}

/// The walk is cargo's own rule — nearest ancestor manifest carrying a
/// `[workspace]` table — so a package deep inside the checkout resolves to the
/// checkout, and the root itself resolves to itself. Both are the starting
/// points cargo gives a test binary: the package root, and the workspace root
/// an agent's shell sits in.
#[test]
fn accept_t437_the_root_is_the_nearest_workspace_manifest_above_the_start() {
    let root = checkout(
        &fixtures_root().join("nearest"),
        &["tools/probe", "crates/probe/src"],
    );

    assert_eq!(
        target_dir::workspace_root_from(&root.join("tools/probe")),
        Some(root.clone()),
        "a package below the checkout must resolve to the checkout's manifest"
    );
    assert_eq!(
        target_dir::workspace_root_from(&root),
        Some(root.clone()),
        "the checkout's own root must resolve to itself"
    );
    assert_eq!(
        target_dir::workspace_root_from(&root.join("crates/probe/src/deeper")),
        Some(root),
        "the walk must keep going up until it finds a workspace manifest"
    );
}

/// The nearest *table* decides, which is what keeps a nested manifest from
/// stealing the walk: a fixture written with its own `[workspace]` table is the
/// root for anything below it, and that is exactly the situation where picking
/// an outer manifest would name the wrong checkout.
#[test]
fn accept_t437_the_nearest_table_wins_when_manifests_nest() {
    let outer = checkout(&fixtures_root().join("nested-outer"), &["crates/probe"]);
    let inner = checkout(&outer.join("vendor/inner"), &["crates/probe"]);

    assert_eq!(
        target_dir::workspace_root_from(&inner.join("crates/probe")),
        Some(inner),
        "an inner workspace manifest is nearer, so it is the root — cargo \
         resolves it the same way"
    );
    assert_eq!(
        target_dir::workspace_root_from(&outer.join("crates/probe")),
        Some(outer),
        "and the outer root is unchanged for its own packages"
    );
}

/// Only the `[workspace]` table makes a manifest a root. A commented-out one, a
/// `workspace = …` key and a `[workspace.dependencies]` table (a different
/// table that merely shares the prefix) must all be passed over, or the walk
/// stops on a manifest cargo would not treat as a root and the gate compares
/// the target directory against a package directory.
#[test]
fn accept_t437_only_a_workspace_table_marks_a_root() {
    // One root for this test, keyed by its name: the three cases rewrite their
    // own subdirectory, and the other test in this binary uses a different
    // root, so no iteration or sibling can delete a tree the other is writing
    // (task #462).
    let base = outside_root("only-a-workspace-table");
    for (name, manifest) in [
        ("commented", "# [workspace]\n[package]\nname = \"probe\"\n"),
        (
            "prefix-only",
            "[package]\nname = \"probe\"\n\n[workspace.dependencies]\n",
        ),
        (
            "keyed",
            "[package]\nname = \"probe\"\n\n[workspace]\nmembers = []\n",
        ),
    ] {
        let root = base.join(format!("table-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("probe")).expect("the fixture dir must be creatable");
        fs::write(root.join("Cargo.toml"), manifest).expect("the manifest must be writable");
        fs::write(
            root.join("probe/Cargo.toml"),
            "[package]\nname = \"probe\"\n",
        )
        .expect("the package manifest must be writable");

        let expected = if name == "keyed" {
            Some(root.clone())
        } else {
            // No ancestor of this fixture is a workspace, so the walk must
            // report that it found none rather than inventing one.
            None
        };
        assert_eq!(
            target_dir::workspace_root_from(&root.join("probe")),
            expected,
            "{name}: a manifest is a workspace root only through its [workspace] \
             table header, found here by cargo"
        );
    }
}

/// `None` — never a guess — when nothing above the start is a workspace root:
/// a missing manifest, an unreadable one, or a directory that does not exist.
/// The gates turn that into a loud failure naming the checkout they could not
/// identify, which is the only honest answer.
#[test]
fn accept_t437_no_workspace_manifest_above_is_reported_not_guessed() {
    let root = outside_root("no-workspace-manifest").join("bare");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the fixture dir must be creatable");
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"probe\"\n")
        .expect("the package manifest must be writable");
    let deep = root.join("src/inner");
    fs::create_dir_all(&deep).expect("the fixture dir must be creatable");

    assert_eq!(
        target_dir::workspace_root_from(&deep),
        None,
        "a tree with no [workspace] manifest anywhere above it names no root"
    );
    assert_eq!(
        target_dir::workspace_root_from(&root.join("src/does-not-exist")),
        None,
        "a start that does not exist has no manifest above it to find"
    );
}

/// The reason the fix exists, end to end through the real gate: with the root
/// named at run time, the checkout's own private target directory verifies, and
/// a directory two live checkouts could write to is still refused loudly. The
/// first half is what the compile-time root broke — under a shared or reused
/// artifact it judged a foreign root and failed a directory nobody had wrong.
/// The second half is what must not be given up to get it.
#[test]
fn accept_t437_the_gate_judges_the_running_checkout_and_still_refuses_a_shared_dir() {
    let root = checkout(&fixtures_root().join("gate"), &["tools/probe"]);

    let private = root.join("target");
    fs::create_dir_all(&private).expect("the private target dir must be creatable");
    let verified = target_dir::verify_workspace_with_env(&root, Some(private.as_os_str()))
        .expect("a target directory inside the checkout is private to it");
    assert_eq!(
        verified.canonicalize().unwrap_or_else(|_| verified.clone()),
        private.canonicalize().expect("the private dir exists"),
        "the gate must report the directory it verified"
    );

    // The fleet layout from the task: two checkouts under one parent, one
    // directory both would compute. Naming this checkout at run time is what
    // lets the refusal name it, and the sibling is what makes the directory
    // shared rather than merely unusual — the same refusal must hold whichever
    // of the two is judged.
    let sibling = checkout(&fixtures_root().join("gate-sibling"), &["tools/probe"]);
    let shared = root
        .parent()
        .and_then(Path::parent)
        .map(|parent| parent.join("gate-target"))
        .expect("the fixture root has a parent");
    for (judged, other) in [(&root, &sibling), (&sibling, &root)] {
        let error = target_dir::verify_workspace_with_env(judged, Some(shared.as_os_str()))
            .expect_err("a directory another live checkout could own must be refused");
        let message = error.to_string();
        for expected in [
            shared.display().to_string(),
            judged.display().to_string(),
            "CARGO_TARGET_DIR".to_string(),
        ] {
            assert!(
                message.contains(&expected),
                "judging {} must refuse a directory {} could also own and name \
                 both {expected:?} so an agent can act on it, got: {message}",
                judged.display(),
                other.display(),
            );
        }
    }
}

/// The root the gates use is the one cargo itself resolves for the working
/// directory, checked through the CLI that agents run. `verify-target-dir`
/// defaults `--workspace-root` to `.`, so when it is pointed at a checkout the
/// two must agree exactly — otherwise the live gates and the command an agent
/// runs by hand would be judging different trees, which is the whole defect.
#[test]
fn accept_t437_verify_target_dir_agrees_with_the_derived_root() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let root = checkout(&fixtures_root().join("cli"), &["tools/probe"]);
    let private = root.join("target");

    let output = Command::new(bin)
        .args(["verify-target-dir", "--workspace-root"])
        .arg(&root)
        .env("CARGO_TARGET_DIR", &private)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        output.status.code(),
        Some(0),
        "verify-target-dir must accept a directory private to the checkout it \
         was pointed at; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let canonical = root.canonicalize().expect("the fixture root exists");
    assert!(
        stdout.contains(&canonical.display().to_string())
            && stdout.contains("is private to worktree"),
        "the command must name the checkout it judged, and it must be the one \
         the walk resolves for that checkout, got: {stdout:?}"
    );

    // The subcommand's own default is `--workspace-root .`, so run it with no
    // argument at all from inside a checkout: it must resolve the checkout it
    // was launched in, not one baked into the binary. This is the agreement the
    // live gates depend on — the binary an agent runs by hand and the gate that
    // runs inside `cargo test --workspace` must mean the same thing by "this
    // worktree", or a green command and a green test are judging different
    // trees.
    let cwd_run = Command::new(bin)
        .arg("verify-target-dir")
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &private)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        cwd_run.status.code(),
        Some(0),
        "with no --workspace-root the command must judge the checkout it runs \
         in, whose target directory is private to it; stderr: {}",
        String::from_utf8_lossy(&cwd_run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cwd_run.stdout).contains(&canonical.display().to_string()),
        "the default must resolve to that same checkout, got: {:?}",
        String::from_utf8_lossy(&cwd_run.stdout)
    );
}
