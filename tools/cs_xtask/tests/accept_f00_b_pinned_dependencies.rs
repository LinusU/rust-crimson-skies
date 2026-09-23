//! F00-B: the compatible dependency pair must stay pinned.
//!
//! `Cargo.lock` pins the exact Bevy/Avian patches, `rust-toolchain.toml`
//! pins the toolchain, and `workspace.package.rust-version` declares the
//! MSRV. `cs_xtask::pins` is the production code that reads and checks them;
//! these tests prove the workspace really pins the intended baseline and that
//! a drift is rejected instead of silently accepted.

use std::path::{Path, PathBuf};

use cs_xtask::pins::{
    INTENDED_BASELINE, PinError, Pins, lockfile_package_version, toolchain_channel,
    workspace_rust_version,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pins() -> Pins {
    Pins::read(&workspace_root())
        .expect("the workspace must ship Cargo.lock and rust-toolchain.toml")
}

/// The workspace pins the exact patches of the intended Bevy 0.19 / Avian3d
/// 0.7 pair and an exact-patch toolchain that satisfies the declared MSRV.
///
/// Observable failure if the pin is removed: a `cargo update` that moves the
/// pair to another minor (or deletes the toolchain pin) makes `verify` return
/// `Err` and this test fail with the offending key and value.
#[test]
fn accept_f00_b_workspace_pins_the_intended_baseline() {
    let pins = pins();

    pins.verify(&INTENDED_BASELINE)
        .expect("the committed pins must match the intended baseline");

    // Exact patches, as the spec requires them pinned in Cargo.lock: bumping
    // either one must be a deliberate edit of the lockfile and this
    // expectation together.
    assert_eq!(pins.bevy, "0.19.1", "unexpected bevy patch pin");
    assert_eq!(pins.avian3d, "0.7.0", "unexpected avian3d patch pin");
    assert_eq!(
        pins.rust_version, INTENDED_BASELINE.rust_version,
        "the workspace MSRV must stay on the intended series"
    );
    assert_eq!(
        pins.toolchain_channel, "1.98.1",
        "rust-toolchain.toml must pin an exact toolchain patch"
    );
}

/// Failure case: a dependency that left the intended minor series is
/// rejected, naming the crate and both versions.
#[test]
fn accept_f00_b_dependency_series_drift_is_rejected() {
    let drifted = Pins {
        bevy: "0.20.3".to_string(),
        ..pins()
    };
    let error = drifted
        .verify(&INTENDED_BASELINE)
        .expect_err("a bevy minor bump must not pass the pin guard");
    assert!(
        matches!(&error, PinError::Mismatch { what, found, .. } if what == "bevy" && found == "0.20.3"),
        "the rejection must name bevy and the found version, got {error:?}"
    );
    assert!(error.to_string().contains("bevy"), "got {error}");

    let drifted = Pins {
        avian3d: "0.8.1".to_string(),
        ..pins()
    };
    let error = drifted
        .verify(&INTENDED_BASELINE)
        .expect_err("an avian minor bump must not pass the pin guard");
    assert!(
        matches!(&error, PinError::Mismatch { what, found, .. } if what == "avian3d" && found == "0.8.1"),
        "the rejection must name avian3d and the found version, got {error:?}"
    );
}

/// Failure cases for the toolchain: a rolling channel is not a pin, and a
/// channel below the declared MSRV is not compatible.
#[test]
fn accept_f00_b_unpinned_or_stale_toolchain_is_rejected() {
    let rolling = Pins {
        toolchain_channel: "stable".to_string(),
        ..pins()
    };
    let error = rolling
        .verify(&INTENDED_BASELINE)
        .expect_err("a rolling channel must not pass the pin guard");
    assert!(
        matches!(&error, PinError::UnpinnedToolchain { channel } if channel == "stable"),
        "got {error:?}"
    );

    let bare = Pins {
        toolchain_channel: "1.98".to_string(),
        ..pins()
    };
    assert!(
        matches!(
            bare.verify(&INTENDED_BASELINE),
            Err(PinError::UnpinnedToolchain { .. })
        ),
        "a major.minor channel leaves the patch unpinned and must be rejected"
    );

    let stale = Pins {
        toolchain_channel: "1.97.0".to_string(),
        ..pins()
    };
    let error = stale
        .verify(&INTENDED_BASELINE)
        .expect_err("a toolchain below the declared MSRV must be rejected");
    assert!(
        matches!(&error, PinError::Mismatch { what, found, .. } if what == "rust-toolchain channel" && found == "1.97.0"),
        "got {error:?}"
    );
}

/// Failure case: a lockfile that no longer contains a baseline package must
/// be reported, not silently treated as "fine".
#[test]
fn accept_f00_b_lockfile_without_the_baseline_package_is_rejected() {
    let lockfile = "version = 4\n\n[[package]]\nname = \"bevy\"\nversion = \"0.20.0\"\n";
    let error = lockfile_package_version(lockfile, "avian3d")
        .expect_err("a missing avian3d entry must be rejected");
    assert_eq!(
        error,
        PinError::MissingPackage {
            name: "avian3d".to_string()
        },
        "the rejection must name the missing package"
    );

    // The same lockfile does resolve bevy, so the parser reads real package
    // blocks rather than always failing.
    assert_eq!(
        lockfile_package_version(lockfile, "bevy").expect("bevy is present"),
        "0.20.0"
    );
}

/// Failure case: manifests without the pinned keys are rejected with the
/// file and key that are missing.
#[test]
fn accept_f00_b_manifests_without_pin_keys_are_rejected() {
    let error = workspace_rust_version("[workspace]\nresolver = \"3\"\n")
        .expect_err("a Cargo.toml without workspace.package.rust-version must be rejected");
    assert_eq!(
        error,
        PinError::MissingKey {
            file: "Cargo.toml [workspace.package]",
            key: "rust-version",
        }
    );

    let error = toolchain_channel("[toolchain]\ncomponents = [\"clippy\"]\n")
        .expect_err("a rust-toolchain.toml without a channel must be rejected");
    assert_eq!(
        error,
        PinError::MissingKey {
            file: "rust-toolchain.toml [toolchain]",
            key: "channel",
        }
    );

    let missing = Pins::read(&workspace_root().join("target/not-a-workspace"))
        .expect_err("reading a workspace without the pin files must fail");
    assert!(
        matches!(&missing, PinError::Io { path } if path.contains("Cargo.lock")),
        "the rejection must name the unreadable pin file, got {missing:?}"
    );
}
