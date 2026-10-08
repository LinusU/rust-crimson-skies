//! #798 `ENGINE-IMAGE-OUTSIDE-INSTALL`: the owner's decrypted engine image is
//! a separate, owner-supplied static-analysis input at `$CS_ENGINE_IMAGE`,
//! never a file of the read-only retail installation.
//!
//! The owner moved the image out of `$CS_GAME_DIR` for good (owner decision
//! 2026-10-08), so `$CS_GAME_DIR` no longer carries it, the installation
//! inventory no longer has a row for it, and nothing may look for it there.
//! What stands in its place is one production helper,
//! `cs_content::coordinates::load_engine_image`, which reads the variable,
//! hashes what it read and refuses each of the three ways that can go wrong:
//! the variable is unset, the file is unreadable, the digest is not the
//! measured one.
//!
//! The three tests that need no environment are written so they run anywhere;
//! the fourth needs the owner's actual image and says so in its `#[ignore]`
//! reason, failing loudly (never skipping) when it is run with `--ignored`
//! and the variable is unset.

use std::ffi::OsStr;
use std::path::PathBuf;

use cs_app::mission_start::{
    ENGINE_IMAGE, ENGINE_IMAGE_SHA256, EngineStateError, engine_state_source,
};
use cs_content::coordinates::{
    ENGINE_IMAGE_ENV_VAR, EngineImage, EngineImageError, load_engine_image, load_engine_image_from,
    original_image_digest,
};
use cs_types::evidence::ContentHash;

/// A path nothing in this test creates, so a read of it must fail.
fn missing_image() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accept_engine_image_outside_install_missing_{}.exe",
        std::process::id()
    ))
}

/// A temporary path this test writes, so a reader can hash wrong bytes.
fn drifted_image() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accept_engine_image_outside_install_drifted_{}.exe",
        std::process::id()
    ))
}

#[test]
fn accept_engine_image_outside_install_the_environment_variable_is_the_only_source() {
    assert_eq!(
        ENGINE_IMAGE_ENV_VAR, "CS_ENGINE_IMAGE",
        "the owner's variable (#798) names the image"
    );

    // With no value the loader refuses instead of falling back to
    // `$CS_GAME_DIR`, and its message names the variable and the image.
    let error = load_engine_image_from(None).expect_err("an unset variable refuses");
    assert!(matches!(error, EngineImageError::Unset), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains(ENGINE_IMAGE_ENV_VAR),
        "the refusal must name the variable: {message}"
    );
    assert!(
        message.contains(ENGINE_IMAGE),
        "the refusal must name the image: {message}"
    );
    assert!(
        !message.contains("inventory"),
        "the refusal must not fall back to an installation inventory: {message}"
    );

    // An empty value is unset too, not a relative path into the installation.
    let error = load_engine_image_from(Some(OsStr::new(""))).expect_err("empty is unset");
    assert!(matches!(error, EngineImageError::Unset), "{error:?}");
}

#[test]
fn accept_engine_image_outside_install_an_unreadable_file_is_refused_by_name() {
    let missing = missing_image();
    let error =
        load_engine_image_from(Some(missing.as_os_str())).expect_err("no such file refuses");
    match &error {
        EngineImageError::Unreadable { path, .. } => assert_eq!(path, &missing),
        other => panic!("an unreadable file is its own case: {other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains(ENGINE_IMAGE_ENV_VAR), "{message}");
    assert!(
        message.contains(&missing.display().to_string()),
        "the refusal must name the path it could not read: {message}"
    );
}

#[test]
fn accept_engine_image_outside_install_a_drifted_image_is_refused_by_name() {
    let path = drifted_image();
    std::fs::write(&path, b"not the owner's decrypted image")
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    let error = load_engine_image_from(Some(path.as_os_str()));
    // Remove the fixture before asserting, so a failing assertion does not
    // leave a file behind.
    let error = match error {
        Err(error) => {
            std::fs::remove_file(&path).ok();
            error
        }
        Ok(_) => {
            std::fs::remove_file(&path).ok();
            panic!("bytes that are not the measured image must be refused");
        }
    };
    match &error {
        EngineImageError::DigestMismatch {
            path: found_path,
            found,
        } => {
            assert_eq!(found_path, &path, "the refusal names the file it read");
            assert_ne!(
                *found,
                original_image_digest(),
                "the fixture's bytes are not the measured image"
            );
        }
        other => panic!("a drifted image is a digest mismatch: {other:?}"),
    }
    let message = error.to_string();
    assert!(
        message.contains(ENGINE_IMAGE_SHA256),
        "the refusal must name the measured digest: {message}"
    );
}

#[test]
fn accept_engine_image_outside_install_the_engine_state_binding_stands_on_the_loaded_image() {
    // `engine_state_source` takes the separately loaded image, so the
    // loader's refusal reaches the binding with `CS_ENGINE_IMAGE` in its
    // prose and no installation inventory is consulted (#798).
    let error = EngineStateError::from(
        load_engine_image_from(None).expect_err("an unset variable refuses"),
    );
    match &error {
        EngineStateError::ImageUnavailable(inner) => {
            assert!(matches!(inner, EngineImageError::Unset), "{inner:?}");
        }
        other => panic!("an unloadable image is unavailable, not a digest: {other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains(ENGINE_IMAGE_ENV_VAR), "{message}");
    assert!(
        !message.contains("inventory"),
        "the refusal must no longer name the installation inventory: {message}"
    );

    // An image carrying any other digest is refused by name, and the message
    // names the measured one so a reader can tell the two apart.
    let found = ContentHash::from_hex(&"cd".repeat(32)).expect("hex");
    let other_image = EngineImage {
        path: PathBuf::from("/owner").join(ENGINE_IMAGE),
        bytes: Vec::new(),
        digest: found,
    };
    let error = engine_state_source(&other_image).expect_err("a different image");
    let message = error.to_string();
    assert!(message.contains(ENGINE_IMAGE_SHA256), "{message}");
    assert!(
        message.contains(&found.to_hex()),
        "the refusal must name the digest it found: {message}"
    );
}

/// The owner's actual image, read through the variable and bound through the
/// production function.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_engine_image_outside_install_the_owner_s_image_loads_from_the_variable() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE must be set for this test");
    assert_eq!(
        image.digest.to_hex(),
        ENGINE_IMAGE_SHA256,
        "the bytes the owner's variable names are the measured image"
    );
    assert!(
        image.bytes.len() > 1_000_000,
        "the full decrypted engine image, {} bytes",
        image.bytes.len()
    );
    assert!(image.path.is_absolute(), "{}", image.path.display());
    // The image is never installation content: when a retail installation is
    // also in scope, its path must not live inside it.
    if let Ok(game_dir) = std::env::var("CS_GAME_DIR") {
        let game_dir = PathBuf::from(game_dir);
        assert!(
            !image.path.starts_with(&game_dir),
            "{} must not be inside the read-only installation {}",
            image.path.display(),
            game_dir.display()
        );
    }

    // And it is the source the campaign airframe's spans stand on.
    let engine = engine_state_source(&image).expect("the measured image names");
    assert_eq!(engine.airframe.container_path(), ENGINE_IMAGE);
    assert_eq!(engine.airframe.install_sha256(), image.digest);
}
