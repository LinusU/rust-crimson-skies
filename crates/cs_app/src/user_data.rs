//! The user-data base directory and the `--profile-dir` flag (F48-E).
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`;
//! `docs/contracts/CLI-EVIDENCE.md` (`--profile-dir`). This is a design, not a
//! claim about the original game: the original stored nothing here that this
//! module reads.
//!
//! [`crate::profile`] maps `(base, origin, kind, id)` to a directory and never
//! chooses the base. This module chooses it:
//!
//! | Platform | Base |
//! | --- | --- |
//! | Windows | `%APPDATA%\CrimsonSkies` |
//! | macOS | `$HOME/Library/Application Support/CrimsonSkies` |
//! | Linux and other Unix | `$XDG_DATA_HOME/crimson-skies`, else `$HOME/.local/share/crimson-skies` |
//!
//! Every input must be an absolute path; a relative or missing variable is an
//! error, never a guess at the current directory. [`default_base`] is pure over
//! an [`Environment`], so each platform's rule is checkable on any host.
//!
//! `--profile-dir <dir>` names a private profile base for an automated run. The
//! run opens the **synthetic** population under it
//! ([`crate::profile::ProfileSession::open_sandbox`]), which is a sibling of the
//! production population and the only one an automated session may open. The
//! value is refused, before anything is created, when it is empty, holds a NUL,
//! contains a `..` component, or is — or lies inside — the player's real base.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use cs_content::save::settings::SettingCatalog;

use crate::profile::{ProfileSession, SessionError};

/// The flag that selects a private profile base.
pub const PROFILE_DIR_FLAG: &str = "--profile-dir";

/// Directory name of the base under the platform's data location.
const APP_DIR: &str = "CrimsonSkies";
/// The same, for platforms whose convention is lowercase.
const APP_DIR_LOWER: &str = "crimson-skies";

/// A host platform family with its own base-directory convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Windows: `%APPDATA%`.
    Windows,
    /// macOS: `~/Library/Application Support`.
    MacOs,
    /// Linux and other Unix: the XDG data directory.
    Linux,
}

impl Platform {
    /// The platform this binary was built for.
    pub const fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// The environment variables the chooser reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// `HOME`.
    pub home: Option<PathBuf>,
    /// `APPDATA` (Windows).
    pub app_data: Option<PathBuf>,
    /// `XDG_DATA_HOME`.
    pub xdg_data_home: Option<PathBuf>,
}

impl Environment {
    /// Reads the process environment. Empty values count as unset.
    pub fn from_process() -> Self {
        let read = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            home: read("HOME"),
            app_data: read("APPDATA"),
            xdg_data_home: read("XDG_DATA_HOME"),
        }
    }
}

/// The player's base directory could not be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BaseError {
    /// The variable the platform needs is unset.
    Missing(&'static str),
    /// The variable is set but not an absolute path.
    NotAbsolute(&'static str),
}

impl fmt::Display for BaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => {
                write!(f, "cannot choose a user-data directory: {name} is unset")
            }
            Self::NotAbsolute(name) => write!(
                f,
                "cannot choose a user-data directory: {name} is not an absolute path"
            ),
        }
    }
}

impl std::error::Error for BaseError {}

fn absolute(value: &Option<PathBuf>, name: &'static str) -> Result<PathBuf, BaseError> {
    match value {
        None => Err(BaseError::Missing(name)),
        Some(path) if path.is_absolute() => Ok(path.clone()),
        Some(_) => Err(BaseError::NotAbsolute(name)),
    }
}

/// The player's real user-data base for `platform`.
///
/// # Errors
///
/// [`BaseError`] when the variable the platform needs is unset or relative.
pub fn default_base(platform: Platform, env: &Environment) -> Result<PathBuf, BaseError> {
    match platform {
        Platform::Windows => Ok(absolute(&env.app_data, "APPDATA")?.join(APP_DIR)),
        Platform::MacOs => Ok(absolute(&env.home, "HOME")?
            .join("Library")
            .join("Application Support")
            .join(APP_DIR)),
        Platform::Linux => {
            // XDG: an unset or relative XDG_DATA_HOME is ignored, not an error.
            if let Some(xdg) = env.xdg_data_home.as_ref().filter(|path| path.is_absolute()) {
                return Ok(xdg.join(APP_DIR_LOWER));
            }
            Ok(absolute(&env.home, "HOME")?
                .join(".local")
                .join("share")
                .join(APP_DIR_LOWER))
        }
    }
}

/// Why a `--profile-dir` value was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDirRefusal {
    /// The value is empty.
    Empty,
    /// The value contains a NUL byte.
    NulByte,
    /// The value has a `..` component.
    ParentTraversal,
    /// The value names an existing file, not a directory.
    NotADirectory(PathBuf),
    /// The value is, or lies inside, the player's real base.
    InsideRealBase(PathBuf),
}

impl fmt::Display for ProfileDirRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PROFILE_DIR_FLAG}: ")?;
        match self {
            Self::Empty => write!(f, "the directory is empty"),
            Self::NulByte => write!(f, "the directory contains a NUL byte"),
            Self::ParentTraversal => write!(f, "the directory contains a `..` component"),
            Self::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            Self::InsideRealBase(path) => write!(
                f,
                "{} is the player's real user-data directory or inside it; \
 automated runs use a private directory",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProfileDirRefusal {}

/// Removes `.` components so two spellings of one directory compare equal.
fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

/// Resolves symlinks in the longest existing prefix of `path`, so a link into
/// the real base is seen for what it is even when the leaf does not exist yet.
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        if let Ok(real) = head.canonicalize() {
            return tail.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (head.file_name().map(|name| name.to_owned()), head.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                head = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Checks a `--profile-dir` value and returns the absolute directory to use.
///
/// A relative value is taken relative to `cwd`. `real_base` is the player's
/// real base when one could be chosen; a value equal to or inside it is
/// refused. Nothing is created or written.
///
/// # Errors
///
/// [`ProfileDirRefusal`] naming why the value is unusable.
pub fn check_profile_dir(
    value: &str,
    cwd: &Path,
    real_base: Option<&Path>,
) -> Result<PathBuf, ProfileDirRefusal> {
    if value.is_empty() {
        return Err(ProfileDirRefusal::Empty);
    }
    if value.contains('\0') {
        return Err(ProfileDirRefusal::NulByte);
    }
    let path = Path::new(value);
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(ProfileDirRefusal::ParentTraversal);
    }
    let dir = normalize(&if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    });
    if dir.exists() && !dir.is_dir() {
        return Err(ProfileDirRefusal::NotADirectory(dir));
    }
    if let Some(real) = real_base {
        let real = normalize(real);
        let inside = |dir: &Path, real: &Path| dir.starts_with(real);
        if inside(&dir, &real)
            || inside(
                &resolve_existing_prefix(&dir),
                &resolve_existing_prefix(&real),
            )
        {
            return Err(ProfileDirRefusal::InsideRealBase(real));
        }
    }
    Ok(dir)
}

/// Takes `--profile-dir <dir>` off the argument vector, keeping the last.
///
/// # Errors
///
/// A message when the flag is the last argument.
pub fn take_flag(args: &mut Vec<String>) -> Result<Option<String>, String> {
    let mut value = None;
    while let Some(at) = args.iter().position(|arg| arg == PROFILE_DIR_FLAG) {
        if at + 1 >= args.len() {
            return Err(format!("{PROFILE_DIR_FLAG} expects a directory"));
        }
        let taken = args.remove(at + 1);
        args.remove(at);
        value = Some(taken);
    }
    Ok(value)
}

/// Why `--profile-dir` could not be honoured.
#[derive(Debug)]
pub enum ProfileDirError {
    /// The value was refused.
    Refused(ProfileDirRefusal),
    /// The sandbox population could not be opened.
    Session(SessionError),
}

impl fmt::Display for ProfileDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::Session(error) => write!(f, "{PROFILE_DIR_FLAG}: {error}"),
        }
    }
}

impl std::error::Error for ProfileDirError {}

/// Validates `value` against the real host base and opens the sandbox session
/// under it.
///
/// # Errors
///
/// [`ProfileDirError::Refused`] before anything is touched, or
/// [`ProfileDirError::Session`] when the synthetic population cannot be opened.
pub fn open_profile_dir(value: &str) -> Result<ProfileSession, ProfileDirError> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let real = default_base(Platform::host(), &Environment::from_process()).ok();
    let dir = check_profile_dir(value, &cwd, real.as_deref()).map_err(ProfileDirError::Refused)?;
    ProfileSession::open_sandbox(&dir, &SettingCatalog::empty()).map_err(ProfileDirError::Session)
}
