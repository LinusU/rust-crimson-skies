//! Task #440: a worktree-private `CARGO_TARGET_DIR` can still serve a *live*
//! sibling worktree.
//!
//! Task #383 made the effective target directory private to the worktree that
//! writes it, and task #433 made it refuse artifacts a checkout that has since
//! been *removed* produced. Neither fires while the other checkout is still
//! there. The shape the owner hit is two worktrees sharing one `target/`:
//! `…/bunny-alpha-1-rev98` built into `…/bunny-alpha-1/target`; while
//! `bunny-alpha-1-rev98` exists, `verify-target-dir` passes, and a `cargo test`
//! in `bunny-alpha-1` can still run `bunny-alpha-1-rev98`'s binary. That is
//! the confusion task #430 was created from.
//!
//! [#433] deliberately left this out: "a recorded `CARGO_MANIFEST_DIR` that
//! exists but belongs to another live checkout" could not be told apart from
//! cargo's own cache, because a registry crate that reads the variable records
//! `$CARGO_HOME/registry/src/…`. This task separates the two by *location*
//! instead of existence: a recorded manifest directory that is neither inside
//! this workspace root nor under cargo's home (`$CARGO_HOME`, else `~/.cargo`)
//! is a foreign checkout, alive or not, and [`target_dir::verify_workspace`]
//! reports it as [`TargetDirError::Foreign`].
//!
//! [#433]: ../../docs/findings/2026-09-30-t433-cargo-reuses-artifacts-of-a-removed-worktree.md

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cs_xtask::target_dir::{self, TargetDirError};

/// The checkout under test, canonicalized: cargo resolves a checkout's own
/// root before it hands `CARGO_MANIFEST_DIR` to rustc, so an unnormalized
/// `tools/cs_xtask/../..` would never compare equal to what it records.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root must exist")
}

/// Root every fixture is created under (inside the gitignored `target/`, so
/// nothing lands in Git and `cargo clean` reaps it).
fn fixtures_root() -> PathBuf {
    workspace_root().join("target/t440-target-dir-fixtures")
}

/// The fixture package: it reads its own source through
/// `env!("CARGO_MANIFEST_DIR")` and prints which checkout it came from, the
/// shape of the reported `cs_formats` artifact.
const PROBE_MAIN: &str = r#"
fn main() {
    println!("OWN:{}", env!("CARGO_MANIFEST_DIR"));
}
"#;

/// Writes the fixture package at `root`. Every checkout is the same package
/// named `probe`, because identical package ids are what make the artifacts
/// collide.
fn probe_checkout(root: &Path) -> PathBuf {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root.join("src")).expect("the fixture src directory must be creatable");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .expect("the fixture manifest must be writable");
    fs::write(root.join("src/main.rs"), PROBE_MAIN).expect("the fixture main.rs must be writable");
    root.to_path_buf()
}

/// `cargo run` in `checkout` with `CARGO_TARGET_DIR` forced to `target_dir`.
fn cargo_run(checkout: &Path, target_dir: &Path) -> Output {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    Command::new(&cargo)
        .current_dir(checkout)
        .args(["run", "--quiet", "--color", "never"])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .output()
        .expect("cargo run must launch")
}

/// Builds the reported shape under `name` and returns
/// `(foreign_checkout, surviving_checkout, shared_target_dir)`.
///
/// Both checkouts stay on disk — that is the whole point of this task, and it
/// is what `#433`'s fixture does *not* do. The foreign checkout builds into the
/// survivor's target directory, so the survivor's target holds a dep-info
/// record naming a live directory outside the survivor.
fn live_pair(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let foreign = probe_checkout(&fixtures_root().join(format!("{name}-foreign")));
    let survivor = probe_checkout(&fixtures_root().join(format!("{name}-survivor")));
    let shared = survivor.join("target");

    let built = cargo_run(&foreign, &shared);
    assert!(
        built.status.success(),
        "the foreign checkout must build into the shared target dir: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(
        String::from_utf8_lossy(&built.stdout).starts_with("OWN:"),
        "the foreign checkout must run its own binary, got: {}",
        String::from_utf8_lossy(&built.stdout)
    );
    assert!(
        foreign.is_dir(),
        "the foreign checkout must still be on disk for this to be the live case"
    );
    (foreign, survivor, shared)
}

/// The acceptance criterion on real cargo: a private target directory that
/// still records a *live* foreign checkout fails the gate with the new
/// verdict, and the message names the directory, the foreign checkout and this
/// worktree. The removed-worktree rule (#433) must not be what fires — the
/// foreign checkout is still there.
#[test]
fn accept_t440_a_live_foreign_checkout_is_refused_and_named() {
    let (foreign, survivor, shared) = live_pair("gate");

    assert!(
        target_dir::recorded_manifest_dirs(&shared).contains(&foreign),
        "the foreign checkout's build must have recorded its CARGO_MANIFEST_DIR"
    );
    assert!(
        target_dir::removed_manifest_dirs(&shared).is_empty(),
        "the foreign checkout exists, so nothing here is a removed worktree"
    );

    let error = target_dir::verify_workspace_with_env(&survivor, Some(shared.as_os_str()))
        .expect_err("a target dir that still serves a live foreign checkout must fail the gate");
    assert!(
        matches!(error, TargetDirError::Foreign { .. }),
        "the rejection must be the foreign-checkout verdict, got {error:?}"
    );

    let message = error.to_string();
    for expected in [
        shared.display().to_string(),
        survivor.display().to_string(),
        foreign.display().to_string(),
        "CARGO_HOME".to_string(),
        "cargo clean --target-dir".to_string(),
        "#440".to_string(),
    ] {
        assert!(
            message.contains(&expected),
            "the rejection must mention {expected:?}, got: {message}"
        );
    }
}

/// The rule's two exclusions: a record inside this workspace root is this
/// checkout's own, and a record under cargo's home is the registry cache any
/// dependency may leave. Only the third kind — an existing directory that is
/// neither — is foreign.
#[test]
fn accept_t440_only_a_live_directory_outside_workspace_and_cargo_home_is_foreign() {
    let root = fixtures_root().join("handwritten-foreign");
    let target = root.join("target");
    let _ = fs::remove_dir_all(&root);
    let deps = target.join("debug/deps");
    fs::create_dir_all(&deps).expect("the fixture deps dir must be creatable");

    let workspace = root.join("this-worktree");
    let ours = workspace.join("crates/probe");
    let foreign = root.join("other-checkout/crates/probe");
    let gone = root.join("deleted-checkout/crates/probe");
    let cargo_home = root.join("cargo-home");
    let registry = cargo_home.join("registry/src/index.crates.io-0/dep-1.0.0");
    fs::create_dir_all(&ours).expect("this checkout's crate dir must be creatable");
    fs::create_dir_all(&foreign).expect("the live foreign crate dir must be creatable");
    fs::create_dir_all(&registry).expect("the fixture registry crate dir must be creatable");

    for (file, dir) in [
        ("ours", &ours),
        ("foreign", &foreign),
        ("gone", &gone),
        ("registry", &registry),
    ] {
        fs::write(
            deps.join(format!("{file}.d")),
            format!(
                "# env-dep:{}={}\n",
                target_dir::MANIFEST_DIR_VAR,
                dir.display()
            ),
        )
        .expect("the dep-info file must be writable");
    }

    assert_eq!(
        target_dir::foreign_manifest_dirs_with_home(&workspace, &target, Some(&cargo_home)),
        BTreeSet::from([foreign.clone()]),
        "only the live directory outside the workspace and cargo home may be foreign"
    );

    // The cargo-home exclusion is what spares the registry crate: point the
    // rule at an unrelated home and the registry record becomes foreign, which
    // is why the rule cannot be "outside this workspace" alone.
    assert!(
        target_dir::foreign_manifest_dirs_with_home(
            &workspace,
            &target,
            Some(&root.join("some-other-cargo-home")),
        )
        .contains(&registry),
        "without the real cargo home the registry record must be reported"
    );
    assert!(
        !target_dir::foreign_manifest_dirs_with_home(&workspace, &target, Some(&cargo_home))
            .contains(&registry),
        "cargo's own cache is not a foreign checkout"
    );

    // A removed record is `removed_manifest_dirs`' evidence, not this rule's.
    assert!(
        target_dir::removed_manifest_dirs(&target).contains(&gone),
        "the deleted checkout is still the removed-worktree rule's"
    );
    assert!(
        !target_dir::foreign_manifest_dirs_with_home(&workspace, &target, Some(&cargo_home))
            .contains(&gone),
        "a directory that is gone belongs to #433, not to this rule"
    );
    assert!(
        !target_dir::foreign_manifest_dirs_with_home(&workspace, &target, Some(&cargo_home))
            .contains(&ours),
        "this workspace's own record is not foreign"
    );
}

/// The command agents run, end to end: `cs_xtask verify-target-dir` exits 1
/// with the fix on a directory that still serves a live foreign checkout, and
/// exits 0 — without naming it — when the only outside record is cargo's own
/// registry cache under `CARGO_HOME`.
#[test]
fn accept_t440_verify_target_dir_command_distinguishes_a_checkout_from_cargo_home() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let (foreign, survivor, shared) = live_pair("cli");

    let failing = Command::new(bin)
        .args(["verify-target-dir", "--workspace-root"])
        .arg(&survivor)
        .env("CARGO_TARGET_DIR", &shared)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a live foreign CARGO_TARGET_DIR must fail the gate, not print success; stderr: {}",
        String::from_utf8_lossy(&failing.stderr)
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    assert!(
        stderr.contains(&shared.display().to_string())
            && stderr.contains(&survivor.display().to_string())
            && stderr.contains(&foreign.display().to_string())
            && stderr.contains("cargo clean --target-dir"),
        "the failure must name the directory, both checkouts and the fix, got: {stderr:?}"
    );

    // The same survivor, a target directory whose only outside record is a
    // registry crate under a fixture CARGO_HOME: the gate passes.
    let cargo_home = fixtures_root().join("cli-cargo-home");
    let registry = cargo_home.join("registry/src/index.crates.io-0/dep-1.0.0");
    let _ = fs::remove_dir_all(&cargo_home);
    fs::create_dir_all(&registry).expect("the fixture registry crate dir must be creatable");
    let registry_target = survivor.join("target-registry");
    let deps = registry_target.join("debug/deps");
    let _ = fs::remove_dir_all(&registry_target);
    fs::create_dir_all(&deps).expect("the fixture deps dir must be creatable");
    fs::write(
        deps.join("registry.d"),
        format!(
            "# env-dep:{}={}\n",
            target_dir::MANIFEST_DIR_VAR,
            registry.display(),
        ),
    )
    .expect("the dep-info file must be writable");

    let passing = Command::new(bin)
        .args(["verify-target-dir", "--workspace-root"])
        .arg(&survivor)
        .env("CARGO_TARGET_DIR", &registry_target)
        .env("CARGO_HOME", &cargo_home)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        passing.status.code(),
        Some(0),
        "a registry record under CARGO_HOME must not fail the gate; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    assert!(
        String::from_utf8_lossy(&passing.stdout).contains("is private to worktree"),
        "the command must report what it verified, got: {}",
        String::from_utf8_lossy(&passing.stdout)
    );
}

/// The live gate: *this* checkout's own target directory must satisfy the new
/// rule too. If a live sibling worktree's artifacts are still in there,
/// `cargo test --workspace` fails here instead of reporting results about a
/// tree that is not the one it ran in.
#[test]
fn accept_t440_this_worktrees_target_dir_holds_no_live_foreign_checkout() {
    let dir =
        target_dir::verify_workspace(&workspace_root()).unwrap_or_else(|error| panic!("{error}"));

    let foreign = target_dir::foreign_manifest_dirs(&workspace_root(), &dir);
    assert!(
        foreign.is_empty(),
        "{} records live directories outside this worktree and cargo's home, so \
its artifacts belong to another checkout (task #440): give each checkout its \
own target directory with cargo clean --target-dir {}",
        dir.display(),
        dir.display()
    );
}
