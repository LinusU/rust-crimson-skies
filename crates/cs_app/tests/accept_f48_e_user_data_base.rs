//! Acceptance scenarios F48-E: the user-data base chooser and `--profile-dir`.
//! Task test prefix: `accept_f48_e_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_app::profile::{
    LibraryOpenError, ProfileDirError, ProfileSession, SessionError, SessionOrigin, population_dir,
};
use cs_app::user_data::{
    BaseError, Environment, Platform, ProfileDirRefusal, check_profile_dir, default_base, take_flag,
};
use cs_content::save::settings::SettingCatalog;
use cs_types::profile::ProfileKind;

fn env(home: Option<&str>, app_data: Option<&str>, xdg: Option<&str>) -> Environment {
    Environment {
        home: home.map(PathBuf::from),
        app_data: app_data.map(PathBuf::from),
        xdg_data_home: xdg.map(PathBuf::from),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("{name}_{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("reusable scratch dir");
    }
    fs::create_dir_all(&dir).expect("scratch dir");
    // `--profile-dir` refuses `..`, so hand out the resolved path.
    dir.canonicalize().expect("resolved scratch dir")
}

#[test]
fn accept_f48_e_each_platform_has_a_base_and_unusable_environments_are_errors() {
    let app_data = if cfg!(windows) {
        "C:\\appdata"
    } else {
        "/appdata"
    };
    assert_eq!(
        default_base(Platform::Windows, &env(None, Some(app_data), None)),
        Ok(PathBuf::from(app_data).join("CrimsonSkies"))
    );
    assert_eq!(
        default_base(Platform::Windows, &env(None, None, None)),
        Err(BaseError::Missing("APPDATA"))
    );
    assert_eq!(
        default_base(Platform::MacOs, &env(Some("/Users/p"), None, None)),
        Ok(PathBuf::from(
            "/Users/p/Library/Application Support/CrimsonSkies"
        ))
    );
    assert_eq!(
        default_base(Platform::Linux, &env(Some("/home/p"), None, None)),
        Ok(PathBuf::from("/home/p/.local/share/crimson-skies"))
    );
    assert_eq!(
        default_base(
            Platform::Linux,
            &env(Some("/home/p"), None, Some("/x/data"))
        ),
        Ok(PathBuf::from("/x/data/crimson-skies"))
    );
    // A relative XDG_DATA_HOME is ignored per the XDG rule.
    assert_eq!(
        default_base(Platform::Linux, &env(Some("/home/p"), None, Some("rel"))),
        Ok(PathBuf::from("/home/p/.local/share/crimson-skies"))
    );
    assert_eq!(
        default_base(Platform::MacOs, &env(None, None, None)),
        Err(BaseError::Missing("HOME"))
    );
    assert_eq!(
        default_base(Platform::MacOs, &env(Some("relative"), None, None)),
        Err(BaseError::NotAbsolute("HOME"))
    );
}

#[test]
fn accept_f48_e_hostile_profile_dir_values_are_refused() {
    let cwd = Path::new("/work");
    let real = Path::new("/home/p/.local/share/crimson-skies");
    assert_eq!(
        check_profile_dir("", cwd, Some(real)),
        Err(ProfileDirRefusal::Empty)
    );
    assert_eq!(
        check_profile_dir("a\0b", cwd, Some(real)),
        Err(ProfileDirRefusal::NulByte)
    );
    for hostile in ["../escape", "ok/../../escape", "/tmp/x/../y"] {
        assert_eq!(
            check_profile_dir(hostile, cwd, Some(real)),
            Err(ProfileDirRefusal::ParentTraversal),
            "{hostile}"
        );
    }
    for real_ish in [
        "/home/p/.local/share/crimson-skies",
        "/home/p/.local/share/crimson-skies/production",
        "/home/p/./.local/share/crimson-skies/",
    ] {
        assert!(
            matches!(
                check_profile_dir(real_ish, cwd, Some(real)),
                Err(ProfileDirRefusal::InsideRealBase(_))
            ),
            "{real_ish}"
        );
    }
    assert_eq!(
        check_profile_dir("private/p1", cwd, Some(real)),
        Ok(PathBuf::from("/work/private/p1"))
    );
}

#[test]
fn accept_f48_e_a_link_into_the_real_base_is_refused() {
    #[cfg(unix)]
    {
        let root = scratch("accept_f48_e_link");
        let real = root.join("real");
        fs::create_dir_all(&real).expect("real base");
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let through = link.join("sub");
        let value = through.to_str().expect("utf8");
        assert!(matches!(
            check_profile_dir(value, &root, Some(&real)),
            Err(ProfileDirRefusal::InsideRealBase(_))
        ));
    }
}

#[test]
fn accept_f48_e_an_automated_run_cannot_reach_the_production_tree() {
    let base = scratch("accept_f48_e_base");
    let catalog = SettingCatalog::empty();
    assert!(matches!(
        ProfileSession::open(
            &base,
            SessionOrigin::Automated,
            ProfileKind::Production,
            &catalog
        ),
        Err(SessionError::Open(LibraryOpenError::Dir(
            ProfileDirError::AutomatedProductionAccess
        )))
    ));
    assert!(!base.join("production").exists(), "nothing was created");
    let production =
        population_dir(&base, SessionOrigin::Interactive, ProfileKind::Production).expect("prod");
    let synthetic =
        population_dir(&base, SessionOrigin::Automated, ProfileKind::Synthetic).expect("synth");
    assert_eq!(production.parent(), synthetic.parent(), "siblings");
    assert!(!synthetic.starts_with(&production));
    assert!(!production.starts_with(&synthetic));
}

#[test]
fn accept_f48_e_take_flag_keeps_the_last_value_and_rejects_a_missing_one() {
    let mut args: Vec<String> = ["--profile-dir", "a", "--ticks", "3", "--profile-dir", "b"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(take_flag(&mut args), Ok(Some("b".to_string())));
    assert_eq!(args, ["--ticks", "3"]);
    let mut dangling = vec!["--profile-dir".to_string()];
    assert!(take_flag(&mut dangling).is_err());
}

fn cs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs"))
}

#[test]
fn accept_f48_e_the_binary_runs_with_a_profile_dir_and_refuses_a_hostile_one() {
    let dir = scratch("accept_f48_e_cli");
    let good = dir.join("private-profile");
    let ok = cs()
        .args(["--profile-dir"])
        .arg(&good)
        .args(["--synthetic", "--headless", "--ticks", "5"])
        .output()
        .expect("run cs");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let stderr = String::from_utf8_lossy(&ok.stderr);
    assert!(
        stderr.contains(&good.join("synthetic").display().to_string()),
        "the run holds the synthetic population under the private base: {stderr}"
    );
    assert!(
        !good.join("production").exists(),
        "production never created"
    );

    let bad = cs()
        .args(["--profile-dir", "../../escape"])
        .args(["--synthetic", "--headless", "--ticks", "5"])
        .output()
        .expect("run cs");
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--profile-dir"));

    let dangling = cs().arg("--profile-dir").output().expect("run cs");
    assert_eq!(dangling.status.code(), Some(2));
}
