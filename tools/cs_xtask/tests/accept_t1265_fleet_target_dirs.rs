//! Task #1265: a cheap scan that no agent checkout's target directory holds
//! artifacts whose recorded `CARGO_MANIFEST_DIR` names a different checkout.
//!
//! The reported defect was fleet-wide: a cargo whose working directory was
//! `bunny-2` wrote build artifacts into `bunny-alpha-2/target`, and the only
//! evidence was the dep-info records the foreign build left behind. The
//! per-worktree gate (`verify-target-dir`, tasks #383/#433/#440) judges only
//! the checkout it runs in; [`target_dir::scan_fleet`] enumerates every
//! checkout under the fleet root — the sibling directories whose own
//! `Cargo.toml` declares a workspace — and applies the same evidence to all
//! of them at once. The provisioning half is covered too: the `CARGO_TARGET_DIR`
//! a checkout's `.env` exports for its sessions must classify as private to
//! that checkout, so a shared directory is caught where it is set rather than
//! where the damage lands.
//!
//! These tests build real fixture checkouts (the same `probe` package shape
//! `accept_t383_`/`accept_t440_` use) under this checkout's `target/`, so the
//! scan exercises cargo's own dep-info records, not fabricated ones, except
//! where a handwritten `.d` is the point of the test.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cs_xtask::target_dir::{self, DeclaredTargetDir};
use cs_xtask::transient;

/// The workspace root of the checkout this test process is *running* in —
/// derived at run time from the working directory, never baked at compile
/// time (task #437); see `accept_t383_`'s comment for the full argument.
fn workspace_root() -> PathBuf {
    target_dir::running_workspace_root().expect("the workspace root must exist")
}

/// Root every fixture fleet is created under (inside the gitignored
/// `target/`, so nothing lands in Git and `cargo clean` reaps it). The
/// per-process subdirectory keeps a second `cargo test` on this target dir
/// from deleting the tree this run is mid-write on (task #610).
fn fixtures_root() -> PathBuf {
    workspace_root().join(format!(
        "target/t1265-fleet-fixtures/{}",
        std::process::id()
    ))
}

/// A fresh fleet root named `name`.
fn fleet(name: &str) -> PathBuf {
    let root = fixtures_root().join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the fixture fleet root must be creatable");
    root
}

/// A minimal cargo package that is its own workspace — the `[workspace]`
/// table is what makes the scan enumerate it as a checkout. The package is
/// named `probe-<name>`, not one shared `probe`: an identical package id in
/// the victim's target directory is *reused* without a rebuild (task #383's
/// silent-reuse defect) and writes no dep-info of its own, while a distinct
/// package is a distinct unit that always compiles fresh and records the
/// checkout it was built from — the record this gate reads.
fn checkout(fleet: &Path, name: &str) -> PathBuf {
    let root = fleet.join(name);
    fs::create_dir_all(root.join("src")).expect("the fixture src directory must be creatable");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n"
        ),
    )
    .expect("the fixture manifest must be writable");
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"CARGO_MANIFEST_DIR\")); }\n",
    )
    .expect("the fixture main.rs must be writable");
    root
}

/// `cargo build` inside `checkout` with `CARGO_TARGET_DIR` forced to
/// `target_dir`; the fixture's `env!("CARGO_MANIFEST_DIR")` makes cargo
/// record the checkout that produced the artifact in its dep-info.
fn cargo_build(checkout: &Path, target_dir: &Path) -> Output {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    Command::new(&cargo)
        .current_dir(checkout)
        .args(["build", "--quiet", "--color", "never"])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .output()
        .expect("cargo build must launch")
}

/// Writes `<root>/.env` with the given content.
fn write_env(root: &Path, content: &str) {
    fs::write(root.join(".env"), content).expect("the fixture .env must be writable");
}

/// Writes a dep-info file `<target>/debug/deps/<name>.d` recording
/// `CARGO_MANIFEST_DIR=<recorded>`, the line cargo emits for a unit whose
/// crate read the variable.
fn dep_file(target: &Path, name: &str, recorded: &Path) {
    let deps = target.join("debug/deps");
    fs::create_dir_all(&deps).expect("the fixture deps dir must be creatable");
    fs::write(
        deps.join(format!("{name}.d")),
        format!(
            "# env-dep:{}={}\n",
            target_dir::MANIFEST_DIR_VAR,
            recorded.display()
        ),
    )
    .expect("the fixture dep-info must be writable");
}

/// The checkout's report inside a fleet report, or panic.
fn checkout_report<'a>(
    report: &'a target_dir::FleetReport,
    root: &Path,
) -> &'a target_dir::CheckoutReport {
    report
        .checkouts
        .iter()
        .find(|checkout| checkout.root == root)
        .unwrap_or_else(|| panic!("{} must be enumerated as a checkout", root.display()))
}

/// The positive side of the criterion: two checkouts each building into
/// their own `target/` scan clean, and every checkout is enumerated — the
/// sibling that holds no workspace manifest is not one.
#[test]
fn accept_t1265_a_clean_fleet_reports_every_checkout_and_no_findings() {
    let fleet = fleet("clean");
    let wt_a = checkout(&fleet, "wt-a");
    let wt_b = checkout(&fleet, "wt-b");
    write_env(&wt_a, "export CARGO_TARGET_DIR=$PWD/target\n");
    for root in [&wt_a, &wt_b] {
        let built = cargo_build(root, &root.join("target"));
        assert!(
            built.status.success(),
            "the fixture build must succeed: {}",
            String::from_utf8_lossy(&built.stderr)
        );
    }
    // Not a checkout: a sibling directory with a target dir but no workspace
    // manifest is not an agent's checkout and must not be scanned as one.
    let stray = fleet.join("not-a-checkout");
    dep_file(&stray.join("target"), "stray", &fleet.join("anywhere"));

    let report = target_dir::scan_fleet(&fleet);
    assert_eq!(
        report
            .checkouts
            .iter()
            .map(|c| c.root.clone())
            .collect::<Vec<_>>(),
        vec![wt_a.clone(), wt_b.clone()],
        "the scan must enumerate exactly the workspace checkouts, in order"
    );
    assert!(
        report.is_clean(),
        "a fleet whose artifacts are all its own must be clean: {:?}",
        report.lines()
    );
    assert_eq!(report.scanned_dirs(), 2);
    assert!(
        matches!(
            checkout_report(&report, &wt_a).declared,
            DeclaredTargetDir::Private(_)
        ),
        "wt-a's $PWD/target declaration is private to wt-a"
    );
    assert!(
        matches!(
            checkout_report(&report, &wt_b).declared,
            DeclaredTargetDir::InCheckoutDefault
        ),
        "wt-b declares nothing, so cargo's in-checkout default applies"
    );
}

/// The reported defect itself: a checkout's cargo wrote into a sibling's
/// `target/`. The scan must name the victim target directory and the foreign
/// checkout the record points at — while that checkout is still present.
#[test]
fn accept_t1265_a_checkout_that_built_into_a_siblings_target_is_named() {
    let fleet = fleet("foreign");
    let victim = checkout(&fleet, "victim");
    let writer = checkout(&fleet, "writer");

    let victim_target = victim.join("target");
    let built = cargo_build(&writer, &victim_target);
    assert!(
        built.status.success(),
        "the foreign build must succeed: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let report = target_dir::scan_fleet(&fleet);
    assert!(
        !report.is_clean(),
        "a target dir holding a foreign checkout's artifacts must fail the scan"
    );
    let victim_report = checkout_report(&report, &victim);
    assert_eq!(
        victim_report.foreign.len(),
        1,
        "exactly one foreign record must be reported"
    );
    let record = &victim_report.foreign[0];
    assert_eq!(
        record.manifest_dir, writer,
        "the record names the foreign checkout"
    );
    assert!(record.live, "the foreign checkout is still on disk");
    assert_eq!(
        record.target_dir,
        victim_target.canonicalize().expect("the target dir exists"),
        "the finding names the victim's target directory"
    );
    let lines = report.lines().join("\n");
    for expected in [
        victim_target.display().to_string(),
        writer.display().to_string(),
        "cargo clean --target-dir".to_string(),
    ] {
        assert!(
            lines.contains(&expected),
            "the report must mention {expected:?}, got: {lines}"
        );
    }
    // The writer's own target-less checkout has nothing to answer for.
    assert!(
        checkout_report(&report, &writer).foreign.is_empty(),
        "the foreign checkout itself records nothing foreign"
    );
}

/// A record whose directory is gone still names a different checkout: the
/// `bunny-alpha-1-rev98` shape, a foreign build whose tree was deleted after
/// it wrote here. Records inside the checkout itself are its own business
/// (`removed_manifest_dirs` gates those per-worktree), and a record under
/// cargo's home is the registry cache — neither may fire here.
#[test]
fn accept_t1265_a_gone_or_cache_or_own_record_is_classified_right() {
    let fleet = fleet("records");
    let victim = checkout(&fleet, "victim");
    let target = victim.join("target");

    let gone_foreign = fleet.join("departed/crates/probe");
    let gone_own = victim.join("long-gone-fixture");
    let cargo_home = fleet.join("cargo-home");
    let registry = cargo_home.join("registry/src/index.crates.io-0/dep-1.0.0");
    fs::create_dir_all(&registry).expect("the fixture registry dir must be creatable");
    dep_file(&target, "foreign", &gone_foreign);
    dep_file(&target, "own", &gone_own);
    dep_file(&target, "registry", &registry);

    let report = target_dir::scan_fleet_with_home(&fleet, Some(&cargo_home));
    let victim_report = checkout_report(&report, &victim);
    assert_eq!(
        victim_report.foreign.len(),
        1,
        "only the record naming a different checkout may be reported: {:?}",
        victim_report.foreign
    );
    let record = &victim_report.foreign[0];
    assert_eq!(record.manifest_dir, gone_foreign);
    assert!(!record.live, "the recorded checkout is gone");
    assert!(
        report.lines().join("\n").contains("recorded path is gone"),
        "a gone record must be described as gone, not as a live checkout"
    );
    assert!(
        !report.is_clean(),
        "a foreign record whose checkout was removed is still a finding"
    );
}

/// The provisioning half of the acceptance: a checkout whose `.env` exports
/// a `CARGO_TARGET_DIR` other checkouts can also write is the defect in
/// waiting, a `$PWD`-relative one is private, and a value the scan cannot
/// expand is reported as a note — never guessed, never silently passed.
#[test]
fn accept_t1265_provisioned_values_are_classified_against_their_checkout() {
    let fleet = fleet("env");
    let shared = checkout(&fleet, "shared-env");
    write_env(
        &shared,
        &format!(
            "export CARGO_TARGET_DIR={}/shared-target\n",
            fleet.display()
        ),
    );
    let private = checkout(&fleet, "private-env");
    write_env(
        &private,
        "export CARGO_TARGET_DIR=$PWD/../targets/private-env\n",
    );
    let commented = checkout(&fleet, "commented-env");
    write_env(
        &commented,
        "# first attempt, now wrong:\nCARGO_TARGET_DIR=/fleet/shared\nexport CARGO_TARGET_DIR=$PWD/target # per-checkout\n",
    );
    let opaque = checkout(&fleet, "opaque-env");
    write_env(&opaque, "export CARGO_TARGET_DIR=$BUILD_ROOT/target\n");

    let report = target_dir::scan_fleet(&fleet);
    assert!(
        matches!(
            checkout_report(&report, &shared).declared,
            DeclaredTargetDir::Shared(_)
        ),
        "a directory every checkout computes identically must be shared"
    );
    assert!(
        matches!(
            checkout_report(&report, &private).declared,
            DeclaredTargetDir::Private(_)
        ),
        "$PWD/../targets/<name> names the checkout below the divergence"
    );
    assert!(
        matches!(
            checkout_report(&report, &commented).declared,
            DeclaredTargetDir::Private(_)
        ),
        "the last assignment wins and a trailing comment must not corrupt the path"
    );
    match &checkout_report(&report, &opaque).declared {
        DeclaredTargetDir::Unverifiable(value) => {
            assert!(
                value.contains("$BUILD_ROOT"),
                "the unverifiable verdict keeps the raw value, got {value:?}"
            );
        }
        other => panic!("an unexpanded variable must be unverifiable, got {other:?}"),
    }
    assert!(
        report
            .notes()
            .iter()
            .any(|note| note.contains("opaque-env")),
        "the unverifiable declaration must be reported as a note"
    );
    // One shared declaration is already enough to fail the fleet.
    assert!(!report.is_clean());
    let lines = report.lines().join("\n");
    assert!(
        lines.contains("shared-env") && lines.contains("shared-target"),
        "the finding must name the checkout and the shared directory, got: {lines}"
    );
}

/// A provisioned directory outside the checkout that is still private by
/// name — the `…/targets/<checkout>` layout — is scanned too: the criterion
/// is about where sessions build, not about the literal `target/` name.
#[test]
fn accept_t1265_a_private_declared_dir_outside_the_checkout_is_scanned() {
    let fleet = fleet("declared-dir");
    let remote = checkout(&fleet, "remote");
    let declared = fleet.join("targets/remote");
    write_env(
        &remote,
        &format!("export CARGO_TARGET_DIR={}\n", declared.display()),
    );
    let other = fleet.join("other/crates/probe");
    fs::create_dir_all(&other).expect("the foreign record's dir must be creatable");
    dep_file(&declared, "foreign", &other);

    let report = target_dir::scan_fleet(&fleet);
    let remote_report = checkout_report(&report, &remote);
    assert!(
        remote_report
            .scanned
            .iter()
            .any(|dir| dir.ends_with("targets/remote")),
        "the provisioned directory must be scanned, got {:?}",
        remote_report.scanned
    );
    assert_eq!(remote_report.foreign.len(), 1);
    assert_eq!(remote_report.foreign[0].manifest_dir, other);
    assert!(remote_report.foreign[0].live);
    assert!(!report.is_clean());
}

/// The command an operator runs, end to end: `cs_xtask verify-target-dirs`
/// exits 0 and says what it covered on a clean fleet, exits 1 naming the
/// finding on a dirty one, and exits 2 on a bad option.
#[test]
fn accept_t1265_verify_target_dirs_command_reports_and_fails_loudly() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let fleet = fleet("cli");
    let wt_a = checkout(&fleet, "wt-a");
    let wt_b = checkout(&fleet, "wt-b");
    for root in [&wt_a, &wt_b] {
        let built = cargo_build(root, &root.join("target"));
        assert!(built.status.success());
    }

    // The fleet root defaults to the workspace root's parent: pointing the
    // command at one checkout scans the fleet it sits in.
    let passing = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dirs", "--workspace-root"])
            .arg(&wt_a),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        passing.status.code(),
        Some(0),
        "a clean fleet must pass; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    let stdout = String::from_utf8_lossy(&passing.stdout);
    for expected in ["wt-a", "wt-b", "checkout(s) under", "scanned"] {
        assert!(
            stdout.contains(expected),
            "the clean report must mention {expected:?}, got: {stdout:?}"
        );
    }

    // A checkout that built into a sibling's target dir fails the gate.
    let built = cargo_build(&wt_b, &wt_a.join("target"));
    assert!(built.status.success());
    let failing = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dirs", "--fleet-root"])
            .arg(&fleet),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a foreign record must fail the fleet gate"
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    for expected in [
        wt_a.join("target")
            .canonicalize()
            .expect("the target dir exists")
            .display()
            .to_string(),
        wt_b.display().to_string(),
        "cargo clean --target-dir".to_string(),
    ] {
        assert!(
            stderr.contains(&expected),
            "the failure must mention {expected:?}, got: {stderr:?}"
        );
    }

    let unusable = transient::command_output(
        Command::new(bin)
            .args(["verify-target-dirs", "--workspace-root"])
            .arg(fleet.join("not-a-workspace")),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        unusable.status.code(),
        Some(1),
        "an unusable workspace root must fail loudly"
    );

    let bad_option =
        transient::command_output(Command::new(bin).args(["verify-target-dirs", "--bogus"]))
            .expect("the cs_xtask binary must run");
    assert_eq!(
        bad_option.status.code(),
        Some(2),
        "an unknown option must be a usage error"
    );
}

/// The live gate: the fleet this checkout sits in must already satisfy the
/// criterion — no sibling's target directory may hold dep-info records
/// naming a different checkout, and no sibling's provisioning may export a
/// `CARGO_TARGET_DIR` that is not private to it. This is what turns "check
/// that no agent's `target/` holds foreign artifacts" from a manual
/// inspection into a scan every `cargo test --workspace` runs. A checkout
/// with no fleet — CI's lone workspace — scans only itself and passes.
#[test]
fn accept_t1265_this_fleets_target_dirs_hold_no_foreign_records() {
    let root = workspace_root();
    let fleet = root
        .parent()
        .expect("the workspace root has a parent to enumerate as the fleet")
        .to_path_buf();
    let report = target_dir::scan_fleet(&fleet);

    assert!(
        report.checkouts.iter().any(|checkout| checkout.root == root
            || checkout.root == root.canonicalize().unwrap_or_else(|_| root.clone())),
        "the fleet scan must cover this checkout itself"
    );
    for note in report.notes() {
        eprintln!("verify-target-dirs note: {note}");
    }
    assert!(
        report.is_clean(),
        "the fleet under {} has target-dir findings — clean the named target \
         directories and fix the named provisioning (task #1265):\n{}",
        fleet.display(),
        report.lines().join("\n")
    );
}
