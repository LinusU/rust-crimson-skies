//! Task #383: every agent worktree needs its own `CARGO_TARGET_DIR`.
//!
//! Cargo keys artifacts by package id and metadata fingerprint, not by
//! source path, so two checkouts building into one `CARGO_TARGET_DIR`
//! overwrite — or worse, silently reuse — each other's artifacts: the
//! `shared-target` case below runs `wt-b`'s own `cargo run` and gets
//! `wt-a`'s marker back. These tests pin the guard
//! (`cs_xtask::target_dir`) that turns that silent clobbering into a loud
//! failure during the required `cargo test --workspace`: the effective
//! directory is taken from `cargo metadata` itself, and it must be private
//! to this worktree. Removing the check — or letting it accept a shared
//! directory — fails them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::target_dir::{self, TargetDirError};
use cs_xtask::transient;

/// The workspace root of the checkout this test process is *running* in, found
/// at run time from the working directory rather than baked in at compile time.
///
/// The gate compares the environment that launched the test, and a compiled
/// test binary can outlive the tree that built it: a `CARGO_TARGET_DIR`
/// shared between checkouts — or one that names a checkout that was since
/// replaced — lets `cargo test` reuse a foreign artifact, and
/// `env!("CARGO_MANIFEST_DIR")` in that artifact names a root this run never
/// touches. Comparing a runtime target directory against a compile-time
/// root misfires: it reports a private `…/f18b/target` as shared because the
/// baked root is `…/devin-1` (task #437). Cargo runs test binaries with the
/// package root as the working directory, so
/// [`target_dir::running_workspace_root`] walks up inside the running
/// checkout regardless of where the binary was compiled. The walk itself is
/// pinned by `accept_t437_` in `accept_t437_runtime_workspace_root.rs`.
fn workspace_root() -> PathBuf {
    target_dir::running_workspace_root().unwrap_or_else(|| {
        panic!(
            "no ancestor of the working directory holds a Cargo.toml with a \
             [workspace] table, so this run cannot name the checkout it is \
             running in; cargo test runs test binaries with the package root \
             as the working directory, so the checkout's own manifest is an \
             ancestor of it"
        )
    })
}

/// Root every fixture worktree is created under (inside the gitignored
/// `target/`, so nothing lands in Git and `cargo clean` reaps it). The
/// per-process subdirectory keeps a second `cargo test` on this target dir
/// from deleting the tree this run is mid-write on (task #610).
fn fixtures_root() -> PathBuf {
    workspace_root().join(format!(
        "target/t383-target-dir-fixtures/{}",
        std::process::id()
    ))
}

/// A minimal cargo package detached from this checkout's workspace whose
/// binary prints `marker`. Every worktree this test creates uses the same
/// package name `probe`, because identical package ids across different
/// checkouts are exactly what makes the shared target dir collide.
fn worktree(name: &str, marker: &str) -> PathBuf {
    let root = fixtures_root().join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("the fixture src directory must be creatable");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .expect("the fixture manifest must be writable");
    fs::write(
        root.join("src/main.rs"),
        format!("fn main() {{ println!(\"{marker}\"); }}\n"),
    )
    .expect("the fixture main.rs must be writable");
    root
}

/// `cargo run --quiet` inside `worktree` with `CARGO_TARGET_DIR` forced to
/// `target_dir`; returns the program's stdout.
fn cargo_run(worktree: &Path, target_dir: &Path) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .current_dir(worktree)
        .args(["run", "--quiet", "--color", "never"])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .output()
        .expect("cargo run must launch");
    assert!(
        output.status.success(),
        "cargo run in {} failed: {}",
        worktree.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("the fixture binary prints UTF-8")
}

/// The defect from the task, reproduced: two worktrees sharing one
/// `CARGO_TARGET_DIR`, and the second one's `cargo run` executes the
/// *first* worktree's binary. `wt-b`'s marker never reaches the binary
/// under test — a check result from such an environment is not evidence
/// about the tree it ran in.
#[test]
fn accept_t383_a_shared_target_dir_runs_a_foreign_binary() {
    let worktree_a = worktree("shared-wt-a", "MARKER-A");
    let worktree_b = worktree("shared-wt-b", "MARKER-B");
    let shared = fixtures_root().join("shared-target");

    assert_eq!(
        cargo_run(&worktree_a, &shared),
        "MARKER-A\n",
        "the first worktree builds and runs its own binary"
    );
    // Cargo reports Finished without compiling: the shared dir already
    // holds a fresh `probe` artifact — worktree A's. B's mutation is
    // invisible in B's own run.
    assert_eq!(
        cargo_run(&worktree_b, &shared),
        "MARKER-A\n",
        "under a shared CARGO_TARGET_DIR the second worktree silently \
reuses the first worktree's binary"
    );
}

/// The acceptance criterion, positive side: with a distinct
/// `CARGO_TARGET_DIR` per worktree the same two builds run at the same
/// time and each binary is the one its own source produced — a mutation
/// in one worktree is visible in that one's binary.
#[test]
fn accept_t383_per_worktree_target_dirs_run_each_own_binary() {
    let worktree_a = worktree("own-wt-a", "MARKER-A");
    let worktree_b = worktree("own-wt-b", "MARKER-B");
    let target_a = fixtures_root().join("targets/own-wt-a");
    let target_b = fixtures_root().join("targets/own-wt-b");

    assert_eq!(cargo_run(&worktree_a, &target_a), "MARKER-A\n");
    assert_eq!(cargo_run(&worktree_b, &target_b), "MARKER-B\n");
    assert_eq!(
        cargo_run(&worktree_a, &target_a),
        "MARKER-A\n",
        "worktree A still runs its own binary after B built"
    );
    assert_eq!(
        cargo_run(&worktree_b, &target_b),
        "MARKER-B\n",
        "worktree B still runs its own binary"
    );
}

/// The worktree-local default and any path below the workspace root are
/// private by construction.
#[test]
fn accept_t383_a_dir_inside_the_worktree_is_accepted() {
    let root = workspace_root();
    assert!(target_dir::is_per_worktree(&root, &root.join("target")));
    assert!(target_dir::is_per_worktree(
        &root,
        &root.join("target/agent-t383")
    ));
}

/// The recommended fleet layout — a shared parent that derives a directory
/// named after the worktree (`$ROOT/target/<worktree>` or `$ROOT/<worktree>`)
/// — is also private, because no other worktree computes the same path.
#[test]
fn accept_t383_a_dir_named_after_the_worktree_is_accepted() {
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace exists");
    let name = root.file_name().expect("the workspace root has a name");
    let parent = root.parent().expect("the workspace root has a parent");

    assert!(target_dir::is_per_worktree(
        &root,
        &parent.join("target").join(name)
    ));
    assert!(target_dir::is_per_worktree(
        &root,
        &parent.join("agent-targets").join(name)
    ));
}

/// The reported defect: every worktree under the fleet root exported the
/// same `…/rust-crimson-skies/target`. A directory that is neither inside
/// the worktree nor named after it fails the check — including a sibling
/// directory belonging to another worktree.
#[test]
fn accept_t383_a_dir_no_other_worktree_could_own_is_rejected() {
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace exists");
    let parent = root.parent().expect("the workspace root has a parent");

    assert!(
        !target_dir::is_per_worktree(&root, &parent.join("target")),
        "the fleet's old shared {} must be rejected",
        parent.join("target").display()
    );
    assert!(
        !target_dir::is_per_worktree(&root, &parent.join("some-other-worktree").join("target")),
        "another worktree's target dir must be rejected"
    );
    assert!(
        !target_dir::is_per_worktree(&root, Path::new("/tmp/cs-shared-target")),
        "a generic path that names no worktree must be rejected"
    );
}

/// Regression for checkouts shaped `…/<name>/<name>` (GitHub Actions
/// checks out into `<repo>/<repo>`, and a worktree may sit under a parent
/// that shares its basename): a directory whose matching component is the
/// worktree's *parent* — not the worktree itself — is still shared by
/// every checkout under that parent. Only components below the point where
/// the path diverges from the workspace may count.
#[test]
fn accept_t383_a_dir_named_like_the_worktrees_parent_is_rejected() {
    let base = fixtures_root().join("same-name");
    let root = base.join("checkout").join("checkout");
    fs::create_dir_all(&root).expect("the fixture root must be creatable");
    let parent = root.parent().expect("the fixture root has a parent");

    assert!(
        !target_dir::is_per_worktree(&root, &parent.join("target")),
        "{} names the worktree only through its parent; every checkout \
under {} shares it",
        parent.join("target").display(),
        parent.display()
    );
    assert!(
        !target_dir::is_per_worktree(&root, &parent.join("another-checkout").join("target")),
        "a sibling checkout's target dir is shared, not this one's"
    );
    // Below the divergence the basename still counts.
    assert!(target_dir::is_per_worktree(
        &root,
        &parent.join("target").join("checkout")
    ));
    assert!(target_dir::is_per_worktree(
        &root,
        &parent.join("targets").join("checkout")
    ));
}

/// The rejection names the variable, the worktree and the fix, so an agent
/// that never noticed the clobbering is told what to do.
#[test]
fn accept_t383_a_shared_dir_fails_with_instructions() {
    let fixture = worktree("shared-env-probe", "MARKER");
    let shared = fixture
        .parent()
        .expect("the fixture has a parent")
        .join("everyone-target");

    let error = target_dir::verify_workspace_with_env(&fixture, Some(shared.as_os_str()))
        .expect_err("a forced shared CARGO_TARGET_DIR must fail the gate");
    let message = error.to_string();
    assert!(
        matches!(error, TargetDirError::Shared { .. }),
        "the rejection must be the shared-dir verdict, got {error:?}"
    );
    for expected in [
        "CARGO_TARGET_DIR",
        "shared-env-probe",
        "Unset",
        "export CARGO_TARGET_DIR",
    ] {
        assert!(
            message.contains(expected),
            "the rejection must mention {expected:?}, got: {message}"
        );
    }
}

/// `effective_target_dir` is Cargo's own answer, not a reimplemented
/// guess: forcing `CARGO_TARGET_DIR` changes it, removing it falls back to
/// `<workspace root>/target`.
#[test]
fn accept_t383_the_effective_dir_tracks_the_environment() {
    let fixture = worktree("env-probe", "MARKER");
    let forced = fixtures_root().join("forced-target");
    fs::create_dir_all(&forced).expect("the forced target dir must be creatable");

    let reported = target_dir::effective_target_dir_with_env(&fixture, Some(forced.as_os_str()))
        .expect("cargo metadata must answer");
    assert_eq!(
        reported.canonicalize().unwrap_or_else(|_| reported.clone()),
        forced.canonicalize().expect("the forced dir exists"),
        "cargo metadata must report the forced CARGO_TARGET_DIR"
    );

    let reported = target_dir::effective_target_dir_with_env(&fixture, None)
        .expect("cargo metadata must answer without the variable too");
    assert_eq!(
        reported,
        fixture
            .canonicalize()
            .expect("the fixture exists")
            .join("target"),
        "with CARGO_TARGET_DIR removed cargo uses the worktree-local target"
    );
}

/// A workspace that cargo cannot read is a loud failure, not a pass.
/// The empty dir must live outside this checkout — inside it, `cargo
/// metadata` correctly walks up and resolves the real workspace instead.
#[test]
fn accept_t383_a_workspace_cargo_cannot_read_fails_loudly() {
    let empty = std::env::temp_dir().join(format!("t383-no-manifest-{}", std::process::id()));
    fs::create_dir_all(&empty).expect("the empty dir must be creatable");
    let error = target_dir::effective_target_dir(&empty)
        .expect_err("cargo metadata without a manifest must fail");
    assert!(
        matches!(
            error,
            TargetDirError::Metadata { .. } | TargetDirError::MetadataShape { .. }
        ),
        "the failure must come from cargo metadata, got {error:?}"
    );
    let _ = fs::remove_dir_all(&empty);
}

/// `target_dir_from_metadata` reads the one field the gate needs and
/// decodes JSON string escapes instead of breaking on paths that contain
/// them.
#[test]
fn accept_t383_metadata_extraction_decodes_json_string_escapes() {
    let json = r#"{"packages":[],"workspace_root":"/w","target_directory":"/fleet/rust-crimson-skies/target"}"#;
    assert_eq!(
        target_dir::target_dir_from_metadata(json).as_deref(),
        Some(Path::new("/fleet/rust-crimson-skies/target"))
    );

    let escaped = r#"{"target_directory":"/w/dir\"quoted\"/slash\\/\u00e9"}"#;
    assert_eq!(
        target_dir::target_dir_from_metadata(escaped).as_deref(),
        Some(Path::new("/w/dir\"quoted\"/slash\\/\u{00e9}"))
    );

    assert!(
        target_dir::target_dir_from_metadata("{\"no_such_field\":1}").is_none(),
        "a missing field is reported, not guessed"
    );
    assert!(
        target_dir::target_dir_from_metadata("{\"target_directory\":42}").is_none(),
        "a non-string field is reported, not guessed"
    );
}

/// The command agents actually run, end to end: `cs_xtask verify-target-dir`
/// exits 0 and names the verified directory when it is worktree-private,
/// exits 1 spelling out the fix when `CARGO_TARGET_DIR` names a directory
/// other checkouts can share, exits 1 on an unusable root and exits 2 on a
/// bad option. Removing the subcommand — or letting it print success on a
/// shared directory — fails this test.
#[test]
fn accept_t383_verify_target_dir_command_reports_and_fails_loudly() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let fixture = worktree("cli-probe", "MARKER");

    // With CARGO_TARGET_DIR removed the fixture resolves its own target/:
    // exit 0, and the command says which directory it verified.
    let passing = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dir", "--workspace-root"])
            .arg(&fixture)
            .env_remove("CARGO_TARGET_DIR"),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        passing.status.code(),
        Some(0),
        "verify-target-dir must pass on a worktree-private target dir; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    let stdout = String::from_utf8_lossy(&passing.stdout);
    assert!(
        stdout.contains("is private to worktree") && stdout.contains("target"),
        "the command must report the directory it verified, got: {stdout:?}"
    );

    // A directory no worktree owns fails the gate with the fix on stderr —
    // never a printed success.
    let shared = fixture
        .parent()
        .expect("the fixture has a parent")
        .join("cli-shared-target");
    let failing = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dir", "--workspace-root"])
            .arg(&fixture)
            .env("CARGO_TARGET_DIR", &shared),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a shared CARGO_TARGET_DIR must fail the gate, not print success"
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    assert!(
        stderr.contains("CARGO_TARGET_DIR") && stderr.contains("cli-probe"),
        "the failure must name the variable and the worktree, got: {stderr:?}"
    );

    let unusable = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dir", "--workspace-root"])
            .arg(fixture.join("target/not-a-workspace")),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        unusable.status.code(),
        Some(1),
        "an unusable workspace root must fail the gate"
    );

    let bad_option =
        transient::command_output(Command::new(bin).args(["verify-target-dir", "--bogus"]))
            .expect("the cs_xtask binary must run");
    assert_eq!(
        bad_option.status.code(),
        Some(2),
        "an unknown option must be a usage error"
    );
}

/// The live gate: *this* checkout's environment must already satisfy the
/// rule — `cargo test --workspace` itself fails loudly on an agent whose
/// `CARGO_TARGET_DIR` is still shared, instead of producing results that
/// cannot be trusted.
#[test]
fn accept_t383_this_worktrees_effective_target_dir_is_per_worktree() {
    let dir =
        target_dir::verify_workspace(&workspace_root()).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        target_dir::is_per_worktree(&workspace_root(), &dir),
        "{} must be private to {}",
        dir.display(),
        workspace_root().display()
    );
}
