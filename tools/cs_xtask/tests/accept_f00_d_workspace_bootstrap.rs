//! F00-D: platform bootstrap evidence and the frozen workspace shape.
//!
//! `specs/F00-workspace-toolchain-and-first-executable.md` requires the
//! deliverable workspace to hold ten members and pins the toolchain. Losing
//! one of them is *silent* in real cargo: measured evidence in
//! `docs/findings/2026-09-23-f00-d-platform-bootstrap-evidence-and-toolchain-freeze.md`
//! shows `cargo test --workspace --locked` exiting 0 with `tools/cs_inspect`
//! removed from `[workspace] members` while `cs_inspect`'s tests never ran.
//!
//! `cs_xtask::bootstrap::verify_workspace` is the production gate that
//! notices. These tests drive it against the real workspace for the positive
//! case and against self-contained fixtures for every failure, so removing
//! the gate — or making it return `Ok` unconditionally — fails them. The
//! minimum acceptance scenario (remove a required workspace member, prove
//! the gate fails) is `accept_f00_d_removing_a_required_member_fails_the_gate`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::bootstrap::{self, BootstrapError, REQUIRED_MEMBERS};
use cs_xtask::ci::{self, CiError, WORKFLOW_PATH};
use cs_xtask::pins::{self, PinError};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Root every synthetic fixture workspace is created under (inside the
/// gitignored `target/`, so nothing lands in Git and `cargo clean` reaps it).
fn fixtures_root() -> PathBuf {
    workspace_root().join("target/f00-d-bootstrap-fixtures")
}

/// A `[package]` manifest for one fixture member.
fn member_manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n")
}

/// The workspace `Cargo.toml` for `members`, shaped like the real one.
fn workspace_manifest(members: &[String]) -> String {
    let listed = members
        .iter()
        .map(|member| format!("    \"{member}\",\n"))
        .collect::<String>();
    format!(
        "[workspace]\nresolver = \"3\"\nmembers = [\n{listed}]\n\n[workspace.package]\n\
version = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.98\"\n"
    )
}

/// A `Cargo.lock` that pins the intended pair (the gate checks series, the
/// exact patches live in the real lockfile).
fn lockfile(bevy: &str, avian3d: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"avian3d\"\nversion = \"{avian3d}\"\n\n\
[[package]]\nname = \"bevy\"\nversion = \"{bevy}\"\n"
    )
}

/// A workflow containing every gate [`ci::REQUIRED_GATES`] demands, built
/// from the same needles the production guard checks so the fixture cannot
/// drift away from the real requirement.
fn workflow() -> String {
    let mut text =
        String::from("name: CI\n\njobs:\n  rust:\n    runs-on: ubuntu-latest\n    steps:\n");
    for (gate, needles) in ci::REQUIRED_GATES {
        text.push_str(&format!(
            "      - name: {gate}\n        run: {}\n",
            needles.join(" ")
        ));
    }
    text
}

/// Creates a complete, passing fixture workspace and returns its root:
/// all ten members with `[package]` manifests, the pinned lockfile, the
/// exact toolchain channel and a gated workflow. Every failure test starts
/// from a fixture that passes, so the assertion is about the mutation.
fn fixture(name: &str) -> PathBuf {
    let root = fixtures_root().join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the fixture root must be creatable");

    let members: Vec<String> = REQUIRED_MEMBERS
        .iter()
        .map(|member| member.to_string())
        .collect();
    fs::write(root.join("Cargo.toml"), workspace_manifest(&members))
        .expect("the fixture manifest must be writable");
    fs::write(root.join("Cargo.lock"), lockfile("0.19.1", "0.7.0"))
        .expect("the fixture lockfile must be writable");
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.98.1\"\ncomponents = [\"rustfmt\", \"clippy\"]\n",
    )
    .expect("the fixture toolchain file must be writable");

    let workflow_path = root.join(WORKFLOW_PATH);
    fs::create_dir_all(workflow_path.parent().expect("the workflow has a parent"))
        .expect("the fixture workflow directory must be creatable");
    fs::write(&workflow_path, workflow()).expect("the fixture workflow must be writable");

    for member in REQUIRED_MEMBERS {
        let directory = root.join(member);
        fs::create_dir_all(&directory).expect("the fixture member directory must be creatable");
        let package_name = member.rsplit('/').next().expect("a member has a name");
        fs::write(directory.join("Cargo.toml"), member_manifest(package_name))
            .expect("the fixture member manifest must be writable");
    }

    assert!(
        bootstrap::verify_workspace(&root).is_ok(),
        "the fresh fixture must pass before it is mutated"
    );
    root
}

/// Rewrites a fixture's `Cargo.toml` with exactly `members` listed.
fn write_members(root: &Path, members: Vec<String>) {
    fs::write(root.join("Cargo.toml"), workspace_manifest(&members))
        .expect("the fixture manifest must be writable");
}

/// Every required member except `removed`.
fn members_except(removed: &str) -> Vec<String> {
    REQUIRED_MEMBERS
        .iter()
        .filter(|member| **member != removed)
        .map(|member| member.to_string())
        .collect()
}

/// The real workspace passes: all ten members listed with manifests, the
/// pins frozen at the intended baseline, CI intact.
///
/// Observable failure if the gate is removed or stubbed: `verify_workspace`
/// no longer exists (compile error) or the report assertions — ten members,
/// exact toolchain patch — fail.
#[test]
fn accept_f00_d_bootstrap_gate_passes_on_this_workspace() {
    let report = bootstrap::verify_workspace(&workspace_root())
        .expect("this workspace must satisfy the platform bootstrap gate");

    for member in REQUIRED_MEMBERS {
        assert!(
            report.members.iter().any(|found| found == member),
            "{member} must be listed in [workspace] members, got {:?}",
            report.members
        );
    }
    report
        .pins
        .verify(&pins::INTENDED_BASELINE)
        .expect("the observed pins must match the intended baseline");
    assert_eq!(
        report.pins.toolchain_channel, "1.98.1",
        "the toolchain must stay frozen at an exact patch"
    );
    assert_eq!(
        report.pins.rust_version,
        pins::INTENDED_BASELINE.rust_version,
        "the workspace MSRV must stay on the intended series"
    );
}

/// **Minimum acceptance scenario:** remove a required workspace member and
/// prove the gate fails — for `tools/cs_inspect` (which cargo drops from the
/// workspace without any `--locked` gate noticing), for `crates/cs_types`
/// (which cargo would silently re-add as an implicit path dependency, so the
/// `members` list itself would keep lying), and for a glob that hides the
/// explicit list the deliverable promises.
///
/// Observable failure if the check is removed: the mutated manifests pass
/// and the `expect_err` calls fail.
#[test]
fn accept_f00_d_removing_a_required_member_fails_the_gate() {
    let root = fixture("member-removed");
    write_members(&root, members_except("tools/cs_inspect"));
    let error = bootstrap::verify_workspace(&root)
        .expect_err("dropping tools/cs_inspect from the members list must fail the gate");
    assert!(
        matches!(
            &error,
            BootstrapError::MissingMember { member } if member == "tools/cs_inspect"
        ),
        "the rejection must name the dropped member, got {error:?}"
    );
    assert!(
        error.to_string().contains("tools/cs_inspect"),
        "the message must be usable as-is, got {error}"
    );

    let root = fixture("member-removed-path-dependency");
    write_members(&root, members_except("crates/cs_types"));
    let error = bootstrap::verify_workspace(&root)
        .expect_err("dropping crates/cs_types from the members list must fail the gate");
    assert!(
        matches!(
            &error,
            BootstrapError::MissingMember { member } if member == "crates/cs_types"
        ),
        "the rejection must name the dropped member, got {error:?}"
    );

    let root = fixture("member-replaced-by-glob");
    write_members(&root, vec!["crates/*".to_string(), "tools/*".to_string()]);
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a glob instead of the explicit member list must fail the gate");
    assert!(
        matches!(&error, BootstrapError::MissingMember { .. }),
        "globs do not satisfy the required members, got {error:?}"
    );
}

/// A member that is listed but unusable is also a broken bootstrap: no
/// manifest at its path, or a manifest that is not a package.
///
/// Observable failure if the check is removed: both mutated fixtures pass.
#[test]
fn accept_f00_d_missing_or_incomplete_member_manifest_is_reported() {
    let root = fixture("manifest-missing");
    fs::remove_file(root.join("crates/cs_net/Cargo.toml"))
        .expect("the fixture manifest must exist to be removed");
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a listed member without a manifest must fail the gate");
    assert!(
        matches!(
            &error,
            BootstrapError::MissingManifest { member, path }
                if member == "crates/cs_net" && path.contains("crates/cs_net")
        ),
        "the rejection must name the member and its missing manifest, got {error:?}"
    );

    let root = fixture("manifest-not-a-package");
    fs::write(root.join("tools/cs_xtask/Cargo.toml"), "[dependencies]\n")
        .expect("the fixture manifest must be writable");
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a member manifest without [package] must fail the gate");
    assert!(
        matches!(
            &error,
            BootstrapError::NotAPackage { member, .. } if member == "tools/cs_xtask"
        ),
        "the rejection must name the member, got {error:?}"
    );
}

/// The composed half of F00-D — the bootstrap stays *frozen*: a missing or
/// rolling toolchain file, a lockfile that left the Bevy 0.19 / Avian3d 0.7
/// pair, and a workflow that lost a CI gate each fail this one gate too.
///
/// Observable failure if composition is removed: the mutated fixtures still
/// return `Ok`.
#[test]
fn accept_f00_d_unfrozen_pins_or_a_lost_ci_gate_fail_the_gate() {
    let root = fixture("toolchain-gone");
    fs::remove_file(root.join("rust-toolchain.toml"))
        .expect("the fixture toolchain file must exist to be removed");
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a workspace without rust-toolchain.toml must fail the gate");
    assert!(
        matches!(&error, BootstrapError::Pin(PinError::Io { path }) if path.contains("rust-toolchain.toml")),
        "the rejection must name the missing toolchain file, got {error:?}"
    );

    let root = fixture("lockfile-drifted");
    fs::write(root.join("Cargo.lock"), lockfile("0.20.0", "0.7.0"))
        .expect("the fixture lockfile must be writable");
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a lockfile on another bevy minor must fail the gate");
    assert!(
        matches!(
            &error,
            BootstrapError::Pin(PinError::Mismatch { what, found, .. })
                if what == "bevy" && found == "0.20.0"
        ),
        "the rejection must name bevy and the drifted version, got {error:?}"
    );

    let root = fixture("workflow-lost-a-gate");
    let without_clippy: String = workflow()
        .lines()
        .filter(|line| !line.contains("cargo clippy"))
        .collect::<Vec<&str>>()
        .join("\n");
    fs::write(root.join(WORKFLOW_PATH), without_clippy)
        .expect("the fixture workflow must be writable");
    let error = bootstrap::verify_workspace(&root)
        .expect_err("a workflow that lost a gate must fail the bootstrap gate");
    assert!(
        matches!(
            &error,
            BootstrapError::Ci(CiError::MissingGate { gate, .. }) if gate.contains("clippy")
        ),
        "the rejection must name the lost gate, got {error:?}"
    );
}

/// The command agents actually run. `cs_xtask verify-bootstrap` passes on
/// this workspace and reports what it froze; it exits 1 naming the dropped
/// member on a broken workspace and on an unusable root; a bad option is a
/// usage error (exit 2). No path may return 0 after printing a failure.
#[test]
fn accept_f00_d_verify_bootstrap_command_reports_and_fails_loudly() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let root = workspace_root();

    let passing = Command::new(bin)
        .args(["verify-bootstrap", "--workspace-root"])
        .arg(&root)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        passing.status.code(),
        Some(0),
        "verify-bootstrap must pass on this workspace; stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    let stdout = String::from_utf8_lossy(&passing.stdout);
    assert!(
        stdout.contains("10 required workspace members"),
        "the command must report the frozen member list, got: {stdout:?}"
    );
    assert!(
        stdout.contains("pins frozen") && stdout.contains("1.98.1"),
        "the command must report the frozen toolchain, got: {stdout:?}"
    );
    assert!(
        stdout.contains("keeps cargo fmt"),
        "the command must report the CI gates it checked, got: {stdout:?}"
    );

    let broken = fixture("cli-member-removed");
    write_members(&broken, members_except("tools/cs_inspect"));
    let failing = Command::new(bin)
        .args(["verify-bootstrap", "--workspace-root"])
        .arg(&broken)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        failing.status.code(),
        Some(1),
        "a missing required member must fail the gate, not print success"
    );
    let stderr = String::from_utf8_lossy(&failing.stderr);
    assert!(
        stderr.contains("tools/cs_inspect") && stderr.contains("not listed"),
        "the failure must name the dropped member, got: {stderr:?}"
    );

    let unusable = Command::new(bin)
        .args(["verify-bootstrap", "--workspace-root"])
        .arg(root.join("target/not-a-workspace"))
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        unusable.status.code(),
        Some(1),
        "an unusable workspace root must fail the gate"
    );
    assert!(
        String::from_utf8_lossy(&unusable.stderr).contains("not a workspace root"),
        "the failure must explain the root, got: {}",
        String::from_utf8_lossy(&unusable.stderr)
    );

    let bad_option = Command::new(bin)
        .args(["verify-bootstrap", "--bogus"])
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        bad_option.status.code(),
        Some(2),
        "an unknown option must be a usage error"
    );
}

/// Cross-check of the static gate against cargo's own view: the committed
/// workspace must really resolve to the ten required members, each with the
/// manifest path the gate checks. This is what makes a `MissingMember`
/// failure meaningful — cargo agrees about who is in the workspace.
///
/// Observable failure if the member list drifts from reality: cargo reports
/// a different set of `manifest_path`s than `REQUIRED_MEMBERS`.
#[test]
fn accept_f00_d_cargo_metadata_sees_every_required_member() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args([
            "metadata",
            "--no-deps",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(workspace_root().join("Cargo.toml"))
        .output()
        .expect("cargo metadata must run");
    assert!(
        output.status.success(),
        "cargo metadata --no-deps --locked must succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        metadata.matches("\"manifest_path\"").count(),
        REQUIRED_MEMBERS.len(),
        "cargo must resolve exactly the required workspace members"
    );
    // cargo reports canonical paths; the manifest dir reaches the root via
    // `tools/cs_xtask/../..`, so compare like with like.
    let root = fs::canonicalize(workspace_root()).expect("the workspace root must be canonical");
    for member in REQUIRED_MEMBERS {
        let manifest = root.join(member).join("Cargo.toml");
        assert!(
            metadata.contains(&manifest.display().to_string()),
            "cargo metadata must report {manifest:?} as a workspace package"
        );
    }
}
