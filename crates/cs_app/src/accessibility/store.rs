//! Atomic settings persistence and the safe-defaults startup (F52-A,
//! non-negotiable behavior 5).
//!
//! [`save_atomic`] writes a sibling temporary file, syncs it and renames it
//! over the target, so a failed or interrupted save leaves the previous file
//! whole. [`startup`] never fails: with the safe flag it ignores the file, and
//! a missing, unreadable or invalid file falls back to the designed defaults
//! with the reason reported. A bad file is left untouched on disk.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cs_content::settings::{Settings, SettingsError};

/// Why a settings file could not be used.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read.
    Io(io::Error),
    /// The file is not valid settings.
    Invalid(SettingsError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Invalid(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LoadError {}

fn temporary(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Validates and atomically replaces the settings file.
///
/// # Errors
///
/// An [`io::Error`] (`InvalidData` for settings that fail validation); the
/// existing file is unchanged.
pub fn save_atomic(path: &Path, settings: &Settings) -> io::Result<()> {
    settings
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temp = temporary(path);
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(settings.to_text().as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Reads and validates the settings file.
///
/// # Errors
///
/// [`LoadError`].
pub fn load(path: &Path) -> Result<Settings, LoadError> {
    let text = fs::read_to_string(path).map_err(LoadError::Io)?;
    Settings::from_text(&text).map_err(LoadError::Invalid)
}

/// Where the startup settings came from.
#[derive(Debug)]
pub enum StartupOrigin {
    /// The file loaded.
    Loaded,
    /// No file existed yet.
    NoFile,
    /// The safe-defaults flag was given; the file was not read.
    SafeFlag,
    /// The file was unusable; defaults were used.
    Recovered(LoadError),
}

/// The settings to start with.
#[derive(Debug)]
pub struct Startup {
    /// The settings.
    pub settings: Settings,
    /// Where they came from.
    pub origin: StartupOrigin,
}

/// Chooses the startup settings; never fails and never writes.
#[must_use]
pub fn startup(path: &Path, safe_defaults: bool) -> Startup {
    let (settings, origin) = if safe_defaults {
        (Settings::designed(), StartupOrigin::SafeFlag)
    } else {
        match load(path) {
            Ok(settings) => (settings, StartupOrigin::Loaded),
            Err(LoadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                (Settings::designed(), StartupOrigin::NoFile)
            }
            Err(error) => (Settings::designed(), StartupOrigin::Recovered(error)),
        }
    };
    Startup { settings, origin }
}
