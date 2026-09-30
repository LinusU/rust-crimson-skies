//! Task #433: a private target directory can still serve a worktree that no
//! longer exists.
//!
//! Task #383 made `CARGO_TARGET_DIR` private to the worktree that writes it,
//! and that is necessary but not sufficient. Cargo keys an artifact by package
//! id and metadata fingerprint, never by source path, so identical content
//! from a different checkout produces an artifact cargo still calls fresh.
//! Concretely, found while verifying task #430: the review worktree
//! `bunny-alpha-1-rev98` was built into `bunny-alpha-1/target` and then
//! deleted, and afterwards `bunny-alpha-1`'s own `cargo test --workspace`
//! failed with
//!
//! ```text
//! reading /…/bunny-alpha-1-rev98/crates/cs_formats/src/pe_resources.rs:
//! No such file or directory (os error 2)
//! ```
//!
//! because `cs_formats` reads its own source through
//! `env!("CARGO_MANIFEST_DIR")`, and the `rlib` cargo reused was the one the
//! deleted worktree compiled. `cargo run -p cs_app --doc` in the same
//! worktree failed differently and from the same cause:
//! `E0432: unresolved import cs_content::world::MissionOverlay`, because the
//! reused `libcs_content-*.rlib` predated the commit that added it.
//!
//! # Reproducing it without any of that
//!
//! Two checkouts of one tree, one target directory, then delete one of them:
//!
//! 1. create `wt-a` and an identical `survivor` from the same sources;
//! 2. build and run `wt-a` with `CARGO_TARGET_DIR=survivor/target`;
//! 3. run `survivor` with that same `CARGO_TARGET_DIR` — cargo reports
//!    `Finished in 0.00s` and executes `wt-a`'s binary;
//! 4. delete `wt-a` and run `survivor` again — still `Finished in 0.00s`,
//!    and the program fails reading a path that is gone.
//!
//! [`accept_t433_a_removed_worktree_leaves_a_live_checkout_running_its_binary`]
//! runs exactly those four steps against real cargo. The resolution is the
//! guard the rest of these tests pin: the `.d` dep-info cargo writes for
//! every unit whose crate read `CARGO_MANIFEST_DIR` records that variable's
//! value, so [`target_dir::removed_manifest_dirs`] can name a checkout that is
//! gone and [`target_dir::verify_workspace`] refuses the directory instead of
//! reporting it as private and trusted.

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
    workspace_root().join("target/t433-target-dir-fixtures")
}

/// The fixture package. Both checkouts below are made from this one source of
/// truth, because *identical content* is what lets cargo reuse the foreign
/// artifact — two checkouts that differ would be rebuilt, which is the case
/// task #383 already covers.
///
/// The binary reads its own source through `env!("CARGO_MANIFEST_DIR")` and
/// reports which checkout it came from, or exits non-zero with the io error,
/// exactly the shape of the reported `cs_formats` failure.
const PROBE_MAIN: &str = r#"
fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = std::path::Path::new(manifest_dir).join("src/main.rs");
    match std::fs::read_to_string(&source) {
        Ok(_) => println!("OWN:{manifest_dir}"),
        Err(error) => {
            eprintln!("cannot read {}: {error}", source.display());
            std::process::exit(1);
        }
    }
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

/// Builds the reported scenario under `name` and returns
/// `(removed_checkout, surviving_checkout, shared_target_dir)` with the
/// removed checkout already deleted.
///
/// Both checkouts are written before the build, so the survivor's sources are
/// older than the artifact — the state in which cargo considers the foreign
/// artifact fresh, on any filesystem whose timestamps resolve in order.
fn stale_pair(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let removed = probe_checkout(&fixtures_root().join(format!("{name}-wt-a")));
    let survivor = probe_checkout(&fixtures_root().join(format!("{name}-survivor")));
    let shared = survivor.join("target");

    let built = cargo_run(&removed, &shared);
    assert!(
        built.status.success(),
        "the first checkout must build into the shared target dir: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(
        String::from_utf8_lossy(&built.stdout).starts_with("OWN:"),
        "the first checkout must run its own binary, got: {}",
        String::from_utf8_lossy(&built.stdout)
    );

    fs::remove_dir_all(&removed).expect("the removed checkout must be deletable");
    (removed, survivor, shared)
}

/// The defect from the task, reproduced end to end: after the first checkout
/// is deleted, the surviving one still runs its binary, and that binary reads
/// a path that no longer exists — while cargo reports `Finished`, so the run
/// looks green. A check result from such an environment is not evidence about
/// the tree it ran in.
#[test]
fn accept_t433_a_removed_worktree_leaves_a_live_checkout_running_its_binary() {
    let (removed, survivor, shared) = stale_pair("repro");

    let reused = cargo_run(&survivor, &shared);
    let stdout = String::from_utf8_lossy(&reused.stdout);
    assert_eq!(
        reused.status.code(),
        Some(1),
        "the surviving checkout must fail the way the report describes; \
stdout: {stdout:?} stderr: {:?}",
        String::from_utf8_lossy(&reused.stderr)
    );
    let removed_src = removed.join("src/main.rs");
    let stderr = String::from_utf8_lossy(&reused.stderr);
    assert!(
        stderr.contains(&removed_src.display().to_string())
            && stderr.contains("No such file or directory"),
        "the failure must name the removed checkout's source, got: {stderr:?}"
    );
    assert!(
        !stdout.contains(&survivor.display().to_string()),
        "the run must not be reading the surviving checkout, got: {stdout:?}"
    );
    assert!(
        !removed.exists(),
        "the first checkout must really be gone for this to be the reported case"
    );
}

/// The acceptance criterion: with a worktree-private target directory that
/// still carries the removed checkout's artifacts, `verify-target-dir`'s
/// library gate fails and its message names the stale directory, the
/// recorded `CARGO_MANIFEST_DIR`, and the fix. It must not report the
/// directory as private and trusted.
#[test]
fn accept_t433_the_gate_fails_and_names_the_stale_directory() {
    let (removed, survivor, shared) = stale_pair("gate");

    let error = target_dir::verify_workspace_with_env(&survivor, Some(shared.as_os_str()))
        .expect_err("a target dir that still serves a removed worktree must fail the gate");
    assert!(
        matches!(error, TargetDirError::Stale { .. }),
        "the rejection must be the stale-dir verdict, got {error:?}"
    );

    let message = error.to_string();
    for expected in [
        shared.display().to_string(),
        removed.display().to_string(),
        "CARGO_MANIFEST_DIR".to_string(),
        "cargo clean --target-dir".to_string(),
        "#433".to_string(),
    ] {
        assert!(
            message.contains(&expected),
            "the rejection must mention {expected:?}, got: {message}"
        );
    }
}

/// The same recorded directory is readable, not guessed: the dep-info cargo
/// wrote names the removed checkout, and only that one is reported gone.
#[test]
fn accept_t433_the_gate_reads_the_recorded_manifest_dir_out_of_dep_info() {
    let (removed, survivor, shared) = stale_pair("record");

    let recorded = target_dir::recorded_manifest_dirs(&shared);
    assert_eq!(
        recorded,
        BTreeSet::from([removed.clone()]),
        "dep-info must record exactly the checkout that built the artifact"
    );
    assert_eq!(
        target_dir::removed_manifest_dirs(&shared),
        BTreeSet::from([removed]),
        "only the directory that is gone may be reported as removed"
    );
    assert!(
        survivor.exists(),
        "the surviving checkout is not the stale directory"
    );
}

/// The positive side: a target directory built only from this checkout is
/// private *and* live, so the gate passes and names it. Removing the
/// staleness check cannot make this test fail; removing it from the gate must
/// not break this one either, which is what keeps the two halves honest.
#[test]
fn accept_t433_a_directory_built_only_from_this_checkout_passes() {
    let survivor = probe_checkout(&fixtures_root().join("live-survivor"));
    let own = survivor.join("target");

    let built = cargo_run(&survivor, &own);
    assert!(
        built.status.success(),
        "the checkout must build into its own target dir: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert_eq!(
        target_dir::recorded_manifest_dirs(&own),
        BTreeSet::from([survivor.clone()]),
        "dep-info must record the checkout that built the artifact"
    );
    assert!(
        target_dir::removed_manifest_dirs(&own).is_empty(),
        "nothing recorded here is gone"
    );

    let verified = target_dir::verify_workspace_with_env(&survivor, Some(own.as_os_str()))
        .expect("a target dir built only from this checkout must pass");
    assert_eq!(verified, own, "the gate reports the directory it verified");
}

/// A hand-written target directory, so the reading rules are pinned without
/// a cargo build: presence-only env-dep lines carry no value, a dependency
/// whose name starts with `env-dep` is not a marker, unreadable layouts are
/// not evidence, and a fresh directory reports nothing.
#[test]
fn accept_t433_dep_info_records_are_read_and_gone_directories_are_named() {
    let root = fixtures_root().join("handwritten");
    let target = root.join("target");
    let _ = fs::remove_dir_all(&root);
    let deps = target.join("debug/deps");
    fs::create_dir_all(&deps).expect("the fixture deps dir must be creatable");
    let gone = root.join("wt-a");
    let alive = root.join("wt-b");
    fs::create_dir_all(&alive).expect("the fixture checkout must be creatable");

    fs::write(
        deps.join("gone.d"),
        format!(
            "/target/debug/deps/gone.d: src/main.rs\n\nsrc/main.rs:\n\n# env-dep:{}=/{}\n",
            target_dir::MANIFEST_DIR_VAR,
            gone.display()
        ),
    )
    .expect("the dep-info file must be writable");
    fs::write(
        deps.join("alive.d"),
        format!(
            "/target/debug/deps/alive.d: src/main.rs\n\nsrc/main.rs:\n\n# env-dep:{}={}\n# env-dep:CLIPPY_CONF_DIR\n",
            target_dir::MANIFEST_DIR_VAR,
            alive.display()
        ),
    )
    .expect("the dep-info file must be writable");
    fs::write(
        deps.join("no-record.d"),
        "/target/debug/deps/no-record.d: src/main.rs\n\nsrc/main.rs:\n\n# env-dep:OUT_DIR=/x/out\n",
    )
    .expect("the dep-info file must be writable");
    // Not a dep-info file, and must not be mistaken for one.
    fs::write(
        deps.join("artifact"),
        format!(
            "# env-dep:{}={}\n",
            target_dir::MANIFEST_DIR_VAR,
            gone.display()
        ),
    )
    .expect("the fixture artifact must be writable");

    assert_eq!(
        target_dir::recorded_manifest_dirs(&target),
        BTreeSet::from([gone.clone(), alive.clone()]),
        "every recorded CARGO_MANIFEST_DIR must be found, and only real dep-info files read"
    );
    assert_eq!(
        target_dir::removed_manifest_dirs(&target),
        BTreeSet::from([gone]),
        "only the directory that is no longer there may be reported"
    );
    assert!(
        target_dir::recorded_manifest_dirs(&root.join("never-built")).is_empty(),
        "a target dir that does not exist records nothing"
    );
    assert!(
        target_dir::recorded_manifest_dirs(&fixtures_root().join("no-such-dir")).is_empty(),
        "an unreadable target dir is not evidence of a removed worktree"
    );

    // The marker has to open the line, and a value has to follow the `=`.
    let dep_info = "target: src/lib.rs env-dep:x.c src/main.rs\n\n# env-dep:CLIPPY_CONF_DIR\n# env-dep:OTHER=1\n";
    assert_eq!(target_dir::env_dep_value(dep_info, "OTHER"), Some("1"));
    assert_eq!(
        target_dir::env_dep_value(dep_info, "CLIPPY_CONF_DIR"),
        None,
        "a presence-only record has no value"
    );
    assert_eq!(
        target_dir::env_dep_value(dep_info, "x"),
        None,
        "a dependency named env-dep is not a record"
    );
    assert_eq!(target_dir::env_dep_value(dep_info, "ABSENT"), None);
}

/// The command agents run, end to end: `cs_xtask verify-target-dir` exits 0
/// on a directory built from this checkout, and exits 1 with the fix on stderr
/// — never a printed success — on one that still serves a removed worktree.
#[test]
fn accept_t433_verify_target_dir_command_fails_on_a_stale_directory() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let (removed, survivor, shared) = stale_pair("cli");

    let failing = Command::new(bin)
        .args(["verify-target-dir", "--workspace-root"])
        .arg(&survivor)
        .env("CARGO_TARGET_DIR", &shared)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a stale CARGO_TARGET_DIR must fail the gate, not print success; stderr: {}",
        String::from_utf8_lossy(&failing.stderr)
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    assert!(
        stderr.contains(&shared.display().to_string())
            && stderr.contains(&removed.display().to_string())
            && stderr.contains("cargo clean --target-dir"),
        "the failure must name the stale directory, the removed worktree and \
the fix, got: {stderr:?}"
    );

    // The same directory with the stale record gone passes and says so.
    fs::remove_dir_all(&shared).expect("the stale artifacts must be deletable");
    let built = cargo_run(&survivor, &survivor.join("target"));
    assert!(
        built.status.success(),
        "the survivor must rebuild into a clean target dir: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let passing = Command::new(bin)
        .args(["verify-target-dir", "--workspace-root"])
        .arg(&survivor)
        .env("CARGO_TARGET_DIR", survivor.join("target"))
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        passing.status.code(),
        Some(0),
        "after the stale artifacts are gone the gate must pass; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    assert!(
        String::from_utf8_lossy(&passing.stdout).contains("is private to worktree"),
        "the command must report what it verified, got: {}",
        String::from_utf8_lossy(&passing.stdout)
    );
}

/// The live gate: *this* checkout's own target directory must satisfy both
/// rules, and everything it records must be this checkout's own crates. If a
/// removed worktree's artifacts are still in there, `cargo test --workspace`
/// fails here instead of reporting results about a tree that is gone.
#[test]
fn accept_t433_this_worktrees_target_dir_holds_no_removed_worktree() {
    let root = workspace_root();
    let dir = target_dir::verify_workspace(&root).unwrap_or_else(|error| panic!("{error}"));

    let recorded = target_dir::recorded_manifest_dirs(&dir);
    assert!(
        !recorded.is_empty(),
        "{} must record at least one CARGO_MANIFEST_DIR, otherwise this \
checkout proves nothing: {}",
        dir.display(),
        target_dir::MANIFEST_DIR_VAR
    );
    for manifest_dir in &recorded {
        assert!(
            manifest_dir.starts_with(&root),
            "{} records {} which belongs to no crate of this worktree",
            dir.display(),
            manifest_dir.display()
        );
        assert!(
            manifest_dir.is_dir(),
            "{} records {}, which is not a directory any more (task #433)",
            dir.display(),
            manifest_dir.display()
        );
    }
}
