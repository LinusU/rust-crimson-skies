//! F61-A acceptance scenarios: the user-data directory policy.
//! Task test prefix: `accept_f61_a_`.
//!
//! Spec: `specs/F61-distribution-installation-ux-notices-and-release-artifacts.md`
//! (deliverable: "creates private cache/save folders"; non-negotiable 4: "Release
//! build startup must work outside the source checkout and without developer
//! absolute paths"), and the open item F48-A left this feature —
//! `docs/findings/2026-10-01-f48-a-profile-and-save-schema.md`:
//! "User-data base directory choice: F61".
//!
//! Every path below is newly authored synthetic data under a platform base. No
//! test touches `$CS_GAME_DIR`, a real profile tree, or a real user's home.

use std::path::{Path, PathBuf};

use cs_xtask::package::{
    APP_DIR_NAME, HostPlatform, UserDataArea, UserDataEnv, UserDataError, UserDataLayout,
    UserDataPolicy,
};

fn windows_env() -> UserDataEnv {
    UserDataEnv {
        app_data: Some(PathBuf::from(r"C:\Users\pilot\AppData\Roaming")),
        home: Some(PathBuf::from(r"C:\Users\pilot")),
        xdg_data_home: None,
    }
}

fn mac_env() -> UserDataEnv {
    UserDataEnv {
        app_data: None,
        home: Some(PathBuf::from("/Users/pilot")),
        xdg_data_home: None,
    }
}

fn linux_env() -> UserDataEnv {
    UserDataEnv {
        app_data: None,
        home: Some(PathBuf::from("/home/pilot")),
        xdg_data_home: Some(PathBuf::from("/home/pilot/.local/share")),
    }
}

/// The base comes from the platform's per-user data location and from nothing
/// else, so a release behaves the same wherever it is unpacked and started.
///
/// Observable failure if the policy is removed: `resolve` takes only a platform
/// and an environment, so this fails rather than passing through a default.
#[test]
fn accept_f61_a_the_user_data_base_follows_the_platform_convention() {
    let policy = UserDataPolicy::release();

    assert_eq!(
        policy.resolve(HostPlatform::Windows, &windows_env()),
        Ok(PathBuf::from(r"C:\Users\pilot\AppData\Roaming").join(APP_DIR_NAME)),
        "Windows keeps per-user data under %APPDATA%"
    );
    assert_eq!(
        policy.resolve(HostPlatform::MacOs, &mac_env()),
        Ok(PathBuf::from("/Users/pilot/Library/Application Support").join(APP_DIR_NAME)),
        "macOS keeps per-user data in Application Support"
    );
    assert_eq!(
        policy.resolve(HostPlatform::Linux, &linux_env()),
        Ok(PathBuf::from("/home/pilot/.local/share").join(APP_DIR_NAME)),
        "Linux honours XDG_DATA_HOME"
    );
    assert_eq!(
        policy
            .resolve(HostPlatform::Linux, &mac_env())
            .expect("HOME is set, so the XDG fallback applies"),
        PathBuf::from("/Users/pilot/.local/share").join(APP_DIR_NAME),
        "with XDG_DATA_HOME unset the XDG default under HOME applies"
    );

    // The answer cannot depend on where the process was started: `resolve` has
    // no argument through which a build path or working directory could enter.
    assert_eq!(HostPlatform::ALL.len(), 3, "every platform is covered");
    for platform in HostPlatform::ALL {
        let env = match platform {
            HostPlatform::Windows => windows_env(),
            HostPlatform::MacOs => mac_env(),
            HostPlatform::Linux => linux_env(),
        };
        let resolved = policy
            .resolve(platform, &env)
            .unwrap_or_else(|error| panic!("{platform} must resolve: {error}"));
        assert!(
            resolved.file_name() == Some(std::ffi::OsStr::new(APP_DIR_NAME)),
            "{platform} must own a directory of its own, got {resolved:?}"
        );
    }
}

/// A missing or relative platform variable is a refusal, not an invented
/// default: guessing where saves go is how a pilot's progress ends up next to
/// the engine.
#[test]
fn accept_f61_a_a_missing_or_relative_platform_variable_is_refused() {
    let policy = UserDataPolicy::release();

    let error = policy
        .resolve(HostPlatform::Windows, &UserDataEnv::default())
        .expect_err("Windows without APPDATA must be refused");
    assert_eq!(
        error,
        UserDataError::MissingEnvironment {
            platform: HostPlatform::Windows,
            variable: "APPDATA",
        }
    );
    assert!(
        error.to_string().contains("APPDATA"),
        "the error must name the variable, got: {error}"
    );

    assert!(
        policy
            .resolve(HostPlatform::MacOs, &UserDataEnv::default())
            .is_err()
    );
    assert!(
        policy
            .resolve(HostPlatform::Linux, &UserDataEnv::default())
            .is_err()
    );

    // A relative value would make the base depend on the working directory.
    let relative = UserDataEnv {
        home: Some(PathBuf::from("relative/home")),
        ..UserDataEnv::default()
    };
    let error = policy
        .resolve(HostPlatform::Linux, &relative)
        .expect_err("a relative HOME must be refused");
    assert!(
        matches!(error, UserDataError::NotAbsolute { .. }),
        "a relative variable must be refused as not absolute, got: {error:?}"
    );

    let relative_xdg = UserDataEnv {
        xdg_data_home: Some(PathBuf::from("./share")),
        ..linux_env()
    };
    assert!(
        policy.resolve(HostPlatform::Linux, &relative_xdg).is_err(),
        "a relative XDG_DATA_HOME must be refused, not honoured"
    );

    // A different app directory name is a policy decision a later stage can
    // make without touching the resolution rules.
    let renamed = UserDataPolicy {
        app_dir_name: "CrimsonSkies".to_string(),
    };
    assert!(renamed.resolve(HostPlatform::MacOs, &mac_env()).is_ok());
}

/// The original installation is read-only and is never written to, and a
/// release cannot assume write access where it was unpacked.
#[test]
fn accept_f61_a_user_data_is_never_inside_the_installation_or_the_application() {
    let policy = UserDataPolicy::release();
    let installation = Path::new("/games/CrimsonSkies");
    let application = Path::new("/opt/releases/crimson-skies-0.1.0");
    let good = Path::new("/home/pilot/.local/share").join(APP_DIR_NAME);

    assert_eq!(
        policy.check(&good, Some(installation), Some(application)),
        Ok(())
    );

    for inside in [
        installation.to_path_buf(),
        installation.join("Data").join("Saves"),
        PathBuf::from("/games/CrimsonSkies2/notes"),
    ] {
        let result = policy.check(&inside, Some(installation), Some(application));
        if inside.starts_with(installation) {
            assert!(
                matches!(result, Err(UserDataError::InsideInstallation { .. })),
                "{inside:?} is inside the installation and must be refused, got {result:?}"
            );
        } else {
            assert!(
                result.is_ok(),
                "{inside:?} is a sibling of the installation, not inside it: a \
                 string-prefix check would wrongly refuse it"
            );
        }
    }

    let inside_application = application.join("data");
    assert!(
        matches!(
            policy.check(&inside_application, Some(installation), Some(application)),
            Err(UserDataError::InsideApplicationDirectory { .. })
        ),
        "a base inside the unpacked release must be refused"
    );

    // Either argument may be unknown. That is not a pass on the other: a caller
    // that does not know where the installation is cannot have this check
    // protect it, which is why F61-C has to pass the installation it resolved.
    assert!(
        policy.check(&good, None, Some(application)).is_ok(),
        "an unknown installation is not a reason to refuse"
    );
    assert!(
        policy
            .check(&installation.join("Data"), Some(installation), None)
            .is_err(),
        "the installation rule does not need the application directory to apply"
    );
    assert!(
        policy
            .check(&inside_application, Some(installation), None)
            .is_ok(),
        "the application rule needs the application directory; this states the limit rather \
         than pretending the first check covered it"
    );
}

/// F48 derives `base/<population>/profile-<id>` and says choosing the base is
/// F61's job, so the layout's profile area *is* the base; the derived cache
/// (F15) and first-run diagnostics sit beside it and can be deleted without
/// touching saves.
#[test]
fn accept_f61_a_the_layout_holds_f48_profiles_cache_and_logs() {
    let layout = UserDataLayout::new(Path::new("/home/pilot/.local/share/CrimsonSkiesRust"));

    assert_eq!(
        layout.area(UserDataArea::Profiles),
        layout.root(),
        "F48's profile base is the user-data base itself"
    );
    assert_eq!(
        layout.area(UserDataArea::Cache),
        Path::new("/home/pilot/.local/share/CrimsonSkiesRust/cache")
    );
    assert_eq!(
        layout.area(UserDataArea::Logs),
        Path::new("/home/pilot/.local/share/CrimsonSkiesRust/logs")
    );
    assert_eq!(layout.directories().len(), UserDataArea::ALL.len());

    // Every area is inside the base: nothing may be handed out outside it.
    for (area, directory) in layout.directories() {
        assert!(
            directory.starts_with(layout.root()),
            "{area} escaped the user-data base: {directory:?}"
        );
    }

    // A base with spaces and non-ASCII characters is a normal base (AC02's
    // shape), and the policy must compose paths for it without rewriting them.
    let awkward = UserDataLayout::new("/Volumes/Bücher & Spiele/CrimsonSkiesRust");
    assert_eq!(
        awkward.area(UserDataArea::Cache),
        PathBuf::from("/Volumes/Bücher & Spiele/CrimsonSkiesRust/cache")
    );

    // And the layout is reachable from the resolution rules themselves.
    let layout = UserDataPolicy::release()
        .layout(HostPlatform::MacOs, &mac_env())
        .expect("macOS resolves");
    assert_eq!(
        layout.root(),
        Path::new("/Users/pilot/Library/Application Support").join(APP_DIR_NAME)
    );
}
