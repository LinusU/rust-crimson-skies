//! The save-write phases, the storage seam, crash-injecting in-memory storage,
//! the commit sequence and recovery.
//!
//! Commit sequence (contract "Persistence"), each step one [`SavePhase`]:
//!
//! 1. [`SavePhase::WriteTemp`]: write the whole new revision to the temp file.
//! 2. [`SavePhase::SyncTemp`]: flush it to stable storage.
//! 3. [`SavePhase::RotateBackup`]: rename a *valid* current file over the
//!    backup. A corrupt current is never rotated, so it cannot replace a good
//!    backup.
//! 4. [`SavePhase::InstallCurrent`]: rename the temp file to current.
//! 5. [`SavePhase::SyncDir`]: sync the directory entry.
//!
//! Recovery reads current, backup and temp, decodes each whole file and takes
//! the one valid file with the highest revision. It never combines fields of
//! two files. A process killed before any phase leaves at least one complete
//! valid revision (the acceptance matrix for every phase is in
//! `tests/accept_f48_a_save_schema.rs`).

use std::fmt;

use cs_types::profile::{ProfileDocument, ProfileId, Revision, SchemaVersion};

use super::codec::{DecodeError, decode, encode};

/// The three files of one profile slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SaveFile {
    Current,
    Backup,
    Temp,
}

impl SaveFile {
    pub const ALL: [Self; 3] = [Self::Current, Self::Backup, Self::Temp];

    /// The file name inside the slot directory.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Current => "profile.sav",
            Self::Backup => "profile.bak",
            Self::Temp => "profile.tmp",
        }
    }
}

/// One step of the write sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SavePhase {
    WriteTemp,
    SyncTemp,
    RotateBackup,
    InstallCurrent,
    SyncDir,
}

impl SavePhase {
    pub const ALL: [Self; 5] = [
        Self::WriteTemp,
        Self::SyncTemp,
        Self::RotateBackup,
        Self::InstallCurrent,
        Self::SyncDir,
    ];
}

/// A storage failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// The process was interrupted inside this phase (injected in tests; a
    /// real crash never returns).
    Interrupted(SavePhase),
    Io(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interrupted(phase) => write!(f, "interrupted during {phase:?}"),
            Self::Io(message) => write!(f, "storage error: {message}"),
        }
    }
}

impl std::error::Error for StorageError {}

/// What a profile slot's storage must provide. Each mutating method is one
/// [`SavePhase`]; the F48-B filesystem implementation maps them to
/// create+write, `fsync`, `rename`, `rename` and directory `fsync`.
pub trait SaveStorage {
    fn read(&self, file: SaveFile) -> Result<Option<Vec<u8>>, StorageError>;
    fn write_temp(&mut self, bytes: &[u8]) -> Result<(), StorageError>;
    fn sync_temp(&mut self) -> Result<(), StorageError>;
    /// Renames current over backup (replacing it).
    fn rotate_backup(&mut self) -> Result<(), StorageError>;
    /// Renames temp over current (replacing it).
    fn install_current(&mut self) -> Result<(), StorageError>;
    fn sync_dir(&mut self) -> Result<(), StorageError>;
}

/// Deterministic in-memory [`SaveStorage`] with crash injection. It is a
/// development storage for tests and the synthetic fixture, never a stand-in
/// for the platform filesystem.
#[derive(Clone, Debug, Default)]
pub struct MemoryStorage {
    current: Option<Vec<u8>>,
    backup: Option<Vec<u8>>,
    temp: Option<Vec<u8>>,
    temp_synced: bool,
    crash_at: Option<SavePhase>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next time `phase` is reached the storage "crashes": the phase's
    /// effect is not applied (a torn half-write for `WriteTemp`, and an
    /// unsynced temp is also truncated to half), and
    /// [`StorageError::Interrupted`] is returned. The crash is one-shot.
    pub fn crash_at(&mut self, phase: SavePhase) {
        self.crash_at = Some(phase);
    }

    pub fn file(&self, file: SaveFile) -> Option<&[u8]> {
        self.slot(file).as_deref()
    }

    /// Replaces a file's bytes, to inject corruption or a pre-existing state.
    pub fn set_file(&mut self, file: SaveFile, bytes: Option<Vec<u8>>) {
        *self.slot_mut(file) = bytes;
    }

    fn slot(&self, file: SaveFile) -> &Option<Vec<u8>> {
        match file {
            SaveFile::Current => &self.current,
            SaveFile::Backup => &self.backup,
            SaveFile::Temp => &self.temp,
        }
    }

    fn slot_mut(&mut self, file: SaveFile) -> &mut Option<Vec<u8>> {
        match file {
            SaveFile::Current => &mut self.current,
            SaveFile::Backup => &mut self.backup,
            SaveFile::Temp => &mut self.temp,
        }
    }

    fn crash(&mut self, phase: SavePhase) -> Result<(), StorageError> {
        if self.crash_at == Some(phase) {
            self.crash_at = None;
            if !self.temp_synced
                && let Some(temp) = &mut self.temp
            {
                temp.truncate(temp.len() / 2);
            }
            return Err(StorageError::Interrupted(phase));
        }
        Ok(())
    }
}

impl SaveStorage for MemoryStorage {
    fn read(&self, file: SaveFile) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.slot(file).clone())
    }

    fn write_temp(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        self.temp_synced = false;
        if self.crash_at == Some(SavePhase::WriteTemp) {
            self.temp = Some(bytes[..bytes.len() / 2].to_vec());
            self.crash_at = None;
            return Err(StorageError::Interrupted(SavePhase::WriteTemp));
        }
        self.temp = Some(bytes.to_vec());
        Ok(())
    }

    fn sync_temp(&mut self) -> Result<(), StorageError> {
        self.crash(SavePhase::SyncTemp)?;
        self.temp_synced = true;
        Ok(())
    }

    fn rotate_backup(&mut self) -> Result<(), StorageError> {
        self.crash(SavePhase::RotateBackup)?;
        if let Some(current) = self.current.take() {
            self.backup = Some(current);
        }
        Ok(())
    }

    fn install_current(&mut self) -> Result<(), StorageError> {
        self.crash(SavePhase::InstallCurrent)?;
        self.current = self.temp.take();
        self.temp_synced = false;
        Ok(())
    }

    fn sync_dir(&mut self) -> Result<(), StorageError> {
        self.crash(SavePhase::SyncDir)
    }
}

/// A visible diagnostic produced by recovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryWarning {
    /// An existing file did not decode and was ignored (not deleted).
    Corrupt { file: SaveFile, error: DecodeError },
    /// The newest state was not in the current file.
    UsedFallback { source: SaveFile },
}

/// The recovered state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub document: ProfileDocument,
    pub source: SaveFile,
    pub warnings: Vec<RecoveryWarning>,
}

/// Why recovery did not produce a state. None of these modify storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoverError {
    /// Files exist but none is valid.
    NoValidSave {
        diagnostics: Vec<(SaveFile, DecodeError)>,
    },
    /// A file is from an unreadable major version. Recovery refuses to fall
    /// back to an older file, so a newer build's data is never shadowed or
    /// overwritten by this build.
    UnsupportedMajor {
        file: SaveFile,
        found: SchemaVersion,
    },
    /// Valid files of one slot name different profiles.
    ProfileMismatch {
        first: ProfileId,
        second: ProfileId,
    },
    Storage(StorageError),
}

impl fmt::Display for RecoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoValidSave { diagnostics } => {
                write!(
                    f,
                    "no valid save among {} unreadable files",
                    diagnostics.len()
                )
            }
            Self::UnsupportedMajor { file, found } => {
                write!(f, "{} has unsupported schema {found}", file.file_name())
            }
            Self::ProfileMismatch { first, second } => {
                write!(f, "slot holds profiles {first} and {second}")
            }
            Self::Storage(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RecoverError {}

impl From<StorageError> for RecoverError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// Read-only recovery. `Ok(None)` means the slot is empty (a fresh profile).
pub fn recover(storage: &dyn SaveStorage) -> Result<Option<Recovery>, RecoverError> {
    let mut valid: Vec<(SaveFile, ProfileDocument)> = Vec::new();
    let mut corrupt: Vec<(SaveFile, DecodeError)> = Vec::new();
    // Preference on equal revisions: current, then temp, then backup.
    for file in [SaveFile::Current, SaveFile::Temp, SaveFile::Backup] {
        let Some(bytes) = storage.read(file)? else {
            continue;
        };
        match decode(&bytes) {
            Ok(document) => valid.push((file, document)),
            Err(DecodeError::UnsupportedMajor { found }) => {
                return Err(RecoverError::UnsupportedMajor { file, found });
            }
            Err(error) => corrupt.push((file, error)),
        }
    }
    let Some(best) = valid
        .iter()
        .enumerate()
        .max_by_key(|(index, (_, doc))| (doc.revision, std::cmp::Reverse(*index)))
        .map(|(index, _)| index)
    else {
        return if corrupt.is_empty() {
            Ok(None)
        } else {
            Err(RecoverError::NoValidSave {
                diagnostics: corrupt,
            })
        };
    };
    let first_id = valid[0].1.profile_id;
    if let Some((_, other)) = valid.iter().find(|(_, doc)| doc.profile_id != first_id) {
        return Err(RecoverError::ProfileMismatch {
            first: first_id,
            second: other.profile_id,
        });
    }
    let (source, document) = valid.swap_remove(best);
    let mut warnings: Vec<RecoveryWarning> = corrupt
        .into_iter()
        .map(|(file, error)| RecoveryWarning::Corrupt { file, error })
        .collect();
    if source != SaveFile::Current {
        warnings.push(RecoveryWarning::UsedFallback { source });
    }
    Ok(Some(Recovery {
        document,
        source,
        warnings,
    }))
}

/// A refused or failed commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitError {
    Encode(cs_types::profile::ProfileFieldError),
    /// The document's schema is not one this build writes.
    UnwritableSchema(SchemaVersion),
    /// The slot cannot be read safely; nothing was written.
    Recover(RecoverError),
    /// The document's revision is not above the stored one (the expected
    /// revision moved): refresh and retry instead of overwriting.
    RevisionConflict {
        stored: Revision,
        offered: Revision,
    },
    /// The slot belongs to a different profile.
    WrongProfile {
        stored: ProfileId,
        offered: ProfileId,
    },
    Storage(StorageError),
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(f, "{error}"),
            Self::UnwritableSchema(v) => write!(f, "schema {v} cannot be written"),
            Self::Recover(error) => write!(f, "{error}"),
            Self::RevisionConflict { stored, offered } => write!(
                f,
                "revision {} is not above stored revision {}",
                offered.0, stored.0
            ),
            Self::WrongProfile { stored, offered } => {
                write!(f, "slot holds profile {stored}, not {offered}")
            }
            Self::Storage(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CommitError {}

impl From<StorageError> for CommitError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// Writes `document` as the new current revision following the phase order
/// above. If a previous commit was interrupted after its temp file was
/// complete, that revision is installed first so it is not discarded.
pub fn commit(
    storage: &mut dyn SaveStorage,
    document: &ProfileDocument,
) -> Result<(), CommitError> {
    if document.schema.major != SchemaVersion::CURRENT.major {
        return Err(CommitError::UnwritableSchema(document.schema));
    }
    let bytes = encode(document).map_err(CommitError::Encode)?;
    let mut existing = recover(storage).map_err(CommitError::Recover)?;
    if let Some(found) = &existing {
        if found.document.profile_id != document.profile_id {
            return Err(CommitError::WrongProfile {
                stored: found.document.profile_id,
                offered: document.profile_id,
            });
        }
        if document.revision <= found.document.revision {
            return Err(CommitError::RevisionConflict {
                stored: found.document.revision,
                offered: document.revision,
            });
        }
    }
    // Finish an interrupted commit whose temp file is the newest state, so
    // writing the next temp cannot destroy the only copy of it.
    if existing
        .as_ref()
        .is_some_and(|r| r.source == SaveFile::Temp)
    {
        rotate_if_current_valid(storage)?;
        storage.install_current()?;
        storage.sync_dir()?;
        existing = recover(storage).map_err(CommitError::Recover)?;
    }
    storage.write_temp(&bytes)?;
    storage.sync_temp()?;
    if existing.is_some_and(|r| r.source == SaveFile::Current) {
        storage.rotate_backup()?;
    }
    storage.install_current()?;
    storage.sync_dir()?;
    Ok(())
}

fn rotate_if_current_valid(storage: &mut dyn SaveStorage) -> Result<(), CommitError> {
    let current_valid = storage
        .read(SaveFile::Current)?
        .is_some_and(|bytes| decode(&bytes).is_ok());
    if current_valid {
        storage.rotate_backup()?;
    }
    Ok(())
}
