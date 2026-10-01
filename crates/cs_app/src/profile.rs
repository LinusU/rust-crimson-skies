//! Profile slot locations, population separation and the runtime library
//! handle (F48-A, F48-B).
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`
//! (non-negotiable 3 and 4). Production, synthetic, modded and evidence
//! profiles live in separate subtrees of one user-data base, and a slot
//! directory is derived only from a [`ProfileKind`] label and the numeric
//! [`ProfileId`], never from a display name, so a name cannot traverse out of
//! the base. An automated session is refused the production subtree outright.
//!
//! Choosing the user-data base itself is F61. This module maps
//! `(base, origin, kind, id)` to a directory and opens the production
//! [`ProfileLibrary`] through that mapping, so the rule that automation never
//! touches a live profile is enforced where the path is produced rather than
//! left to every caller to remember. The library itself does not know about
//! sessions: it is handed one population's directory and does IO there.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_content::save::fs::SlotError;
use cs_content::save::library::{LibraryError, ProfileLibrary};
use cs_types::profile::{ProfileId, ProfileKind};

/// Who is asking for a profile location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionOrigin {
    /// A person playing.
    Interactive,
    /// Tests, evidence harnesses and other automation.
    Automated,
}

/// Refusal to hand out a location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDirError {
    /// Automation asked for the live production profile tree.
    AutomatedProductionAccess,
}

impl fmt::Display for ProfileDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutomatedProductionAccess => {
                write!(
                    f,
                    "automated sessions may not use the production profile tree"
                )
            }
        }
    }
}

impl std::error::Error for ProfileDirError {}

/// The directory holding one profile's `profile.sav`/`.bak`/`.tmp`.
pub fn slot_dir(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
    id: ProfileId,
) -> Result<PathBuf, ProfileDirError> {
    if origin == SessionOrigin::Automated && kind == ProfileKind::Production {
        return Err(ProfileDirError::AutomatedProductionAccess);
    }
    Ok(base.join(kind.label()).join(format!("profile-{id}")))
}

/// The root of one population's subtree: where its registry and its profile
/// slots live.
pub fn population_dir(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
) -> Result<PathBuf, ProfileDirError> {
    // The same refusal as `slot_dir`, and for the same reason: the population
    // root contains every slot, so a session that may not open a production
    // slot may not open the production root either.
    if origin == SessionOrigin::Automated && kind == ProfileKind::Production {
        return Err(ProfileDirError::AutomatedProductionAccess);
    }
    Ok(base.join(kind.label()))
}

/// Why a population's library could not be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryOpenError {
    /// Automation asked for the live production profile tree.
    Dir(ProfileDirError),
    /// The directory or a file in it could not be used.
    Slot(SlotError),
    /// The registry, a slot or a write failed.
    Library(LibraryError),
}

impl fmt::Display for LibraryOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dir(error) => write!(f, "{error}"),
            Self::Slot(error) => write!(f, "{error}"),
            Self::Library(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LibraryOpenError {}

impl From<ProfileDirError> for LibraryOpenError {
    fn from(error: ProfileDirError) -> Self {
        Self::Dir(error)
    }
}

impl From<SlotError> for LibraryOpenError {
    fn from(error: SlotError) -> Self {
        Self::Slot(error)
    }
}

impl From<LibraryError> for LibraryOpenError {
    fn from(error: LibraryError) -> Self {
        Self::Library(error)
    }
}

/// Opens one population's profile library, enforcing the population rule at the
/// point the path is produced.
///
/// The directory is created if it is absent, and the registry slot is recovered
/// on the way in; a registry that does not decode is an error rather than an
/// empty library, because an unreadable high-water mark read as zero is what
/// would reissue a deleted profile id.
pub fn open_library(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
) -> Result<ProfileLibrary, LibraryOpenError> {
    let directory = population_dir(base, origin, kind)?;
    Ok(ProfileLibrary::open(directory, kind)?)
}
