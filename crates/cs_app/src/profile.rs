//! Profile slot locations and population separation (F48-A).
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`
//! (non-negotiable 3 and 4). Production, synthetic, modded and evidence
//! profiles live in separate subtrees of one user-data base, and a slot
//! directory is derived only from a [`ProfileKind`] label and the numeric
//! [`ProfileId`], never from a display name, so a name cannot traverse out of
//! the base. An automated session is refused the production subtree outright.
//!
//! Choosing the user-data base itself is F61; this module only maps
//! `(base, kind, id)` to a directory and performs no IO.

use std::fmt;
use std::path::{Path, PathBuf};

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
