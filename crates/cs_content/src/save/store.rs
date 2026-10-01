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

use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision, SchemaVersion};

use super::codec::{DecodeError, decode, encode};

/// The file name prefix of a profile's three save files.
pub const PROFILE_PREFIX: &str = "profile";

/// The three files of one save slot. A slot holds three files of the same
/// document kind — a profile's save, or a population's registry — so a
/// recovery rule is stated once and applies to both.
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

    /// The same file inside a slot whose document is spelled with `prefix`, so
    /// a slot's name is derived from its subject and never the reverse.
    pub fn prefixed_name(self, prefix: &str) -> String {
        let suffix = match self {
            Self::Current => "sav",
            Self::Backup => "bak",
            Self::Temp => "tmp",
        };
        format!("{prefix}.{suffix}")
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

impl fmt::Display for RecoveryWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt { file, error } => {
                write!(
                    f,
                    "{} did not decode and was ignored: {error}",
                    file.file_name()
                )
            }
            Self::UsedFallback { source } => write!(
                f,
                "the newest state was recovered from {} instead of {}",
                source.file_name(),
                SaveFile::Current.file_name()
            ),
        }
    }
}

/// The recovered state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub document: ProfileDocument,
    pub source: SaveFile,
    pub warnings: Vec<RecoveryWarning>,
}

impl Recovery {
    /// The warnings as text, so a caller that owns a display surface can show
    /// what recovery had to do instead of logging it (F48 AC02: a recovered
    /// backup is never silent).
    pub fn warning_lines(&self) -> Vec<String> {
        self.warnings.iter().map(ToString::to_string).collect()
    }
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
    /// The registry in a population's directory belongs to another
    /// population. It is not adopted and not overwritten.
    RegistryKindMismatch {
        stored: ProfileKind,
        offered: ProfileKind,
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
            Self::RegistryKindMismatch { stored, offered } => write!(
                f,
                "the registry belongs to the {} population, not {}",
                stored.label(),
                offered.label()
            ),
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

/// What a slot holds, every file read and decoded whole.
///
/// This is the contract's selection rule, stated once for every document kind
/// that has a slot (a profile's save, the profile registry): read each file
/// whole, refuse an unreadable schema outright rather than shadow it, and take
/// the valid file with the highest revision — `Current` first on a tie, because
/// the read order above is the preference order. Two files are never combined:
/// the winner is one whole file or nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Survey<T> {
    /// The files that decoded, in read order, with the revision each declares.
    pub valid: Vec<(SaveFile, Revision, T)>,
    /// The files that did not decode, with why. They are kept, never deleted.
    pub corrupt: Vec<(SaveFile, DecodeError)>,
}

impl<T> Survey<T> {
    /// The index of the winning file, or `None` when no file decoded.
    pub fn best(&self) -> Option<usize> {
        self.valid
            .iter()
            .enumerate()
            .max_by(
                |(left, (_, left_revision, _)), (right, (_, right_revision, _))| {
                    left_revision
                        .cmp(right_revision)
                        // Ties go to the earlier file in read order, so the
                        // greatest index wins only by holding a higher revision.
                        .then_with(|| right.cmp(left))
                },
            )
            .map(|(index, _)| index)
    }
}

/// Reads every file of a slot and decodes each one whole with `decode`, which
/// reports the document's revision alongside it.
pub fn survey<T>(
    storage: &dyn SaveStorage,
    decode: impl Fn(&[u8]) -> Result<(Revision, T), DecodeError>,
) -> Result<Survey<T>, RecoverError> {
    let mut valid = Vec::new();
    let mut corrupt = Vec::new();
    for file in [SaveFile::Current, SaveFile::Temp, SaveFile::Backup] {
        let Some(bytes) = storage.read(file)? else {
            continue;
        };
        match decode(&bytes) {
            Ok((revision, value)) => valid.push((file, revision, value)),
            Err(DecodeError::UnsupportedMajor { found }) => {
                return Err(RecoverError::UnsupportedMajor { file, found });
            }
            Err(error) => corrupt.push((file, error)),
        }
    }
    Ok(Survey { valid, corrupt })
}

/// Read-only recovery. `Ok(None)` means the slot is empty (a fresh profile).
pub fn recover(storage: &dyn SaveStorage) -> Result<Option<Recovery>, RecoverError> {
    let mut survey = survey(storage, |bytes| {
        decode(bytes).map(|document| (document.revision, document))
    })?;
    let Some(best) = survey.best() else {
        return if survey.corrupt.is_empty() {
            Ok(None)
        } else {
            Err(RecoverError::NoValidSave {
                diagnostics: survey.corrupt,
            })
        };
    };
    let first_id = survey.valid[0].2.profile_id;
    if let Some((_, _, other)) = survey
        .valid
        .iter()
        .find(|(_, _, doc)| doc.profile_id != first_id)
    {
        return Err(RecoverError::ProfileMismatch {
            first: first_id,
            second: other.profile_id,
        });
    }
    let (source, _, document) = survey.valid.swap_remove(best);
    let mut warnings: Vec<RecoveryWarning> = survey
        .corrupt
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
    /// Two valid files of one slot describe different subjects, so neither is
    /// a continuation of the other.
    SlotIdentityMismatch {
        file: SaveFile,
        stored: String,
        offered: String,
    },
    /// The revision counter cannot advance, so nothing further can be written
    /// without a revision that is not higher than what is stored.
    RevisionExhausted,
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
            Self::SlotIdentityMismatch {
                file,
                stored,
                offered,
            } => write!(
                f,
                "{} describes {offered} while the slot holds {stored}",
                file.file_name()
            ),
            Self::RevisionExhausted => write!(f, "the revision counter cannot advance"),
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

/// The five phases above, on bytes that are already encoded, given what
/// [`survey`] found in the slot.
///
/// Shared with every other persisted document that has a slot, so there is one
/// write sequence rather than one per document type: write the whole new
/// revision to temp, sync it, rotate a *valid* current over the backup, install
/// the temp as current, sync the directory. An interrupted commit whose temp
/// file is the newest whole state is finished first, so writing the next temp
/// cannot destroy the only copy of it.
pub fn run_phases<T>(
    storage: &mut dyn SaveStorage,
    bytes: &[u8],
    survey: Survey<T>,
    decode: &impl Fn(&[u8]) -> Result<(Revision, T), DecodeError>,
) -> Result<(), CommitError> {
    let mut survey = survey;
    if survey
        .best()
        .is_some_and(|index| survey.valid[index].0 == SaveFile::Temp)
    {
        // A corrupt current is never rotated, so it cannot replace a good
        // backup: the rotation is conditional on the current file decoding.
        if storage
            .read(SaveFile::Current)?
            .is_some_and(|held| decode(&held).is_ok())
        {
            storage.rotate_backup()?;
        }
        storage.install_current()?;
        storage.sync_dir()?;
        survey = self::survey(storage, decode).map_err(CommitError::Recover)?;
    }
    storage.write_temp(bytes)?;
    storage.sync_temp()?;
    if survey
        .best()
        .is_some_and(|index| survey.valid[index].0 == SaveFile::Current)
    {
        storage.rotate_backup()?;
    }
    storage.install_current()?;
    storage.sync_dir()?;
    Ok(())
}

/// Writes `document` as the new current revision following the phase order
/// above. A revision that is not above the stored one, or a slot that holds
/// another profile, is refused rather than written over.
pub fn commit(
    storage: &mut dyn SaveStorage,
    document: &ProfileDocument,
) -> Result<(), CommitError> {
    if document.schema.major != SchemaVersion::CURRENT.major {
        return Err(CommitError::UnwritableSchema(document.schema));
    }
    let bytes = encode(document).map_err(CommitError::Encode)?;
    let survey = survey(storage, |held| {
        decode(held).map(|decoded| (decoded.revision, decoded))
    })
    .map_err(CommitError::Recover)?;
    // Every valid file of a profile slot must be that profile's, so recovery
    // and a commit agree on which slot this is. A slot holding two profiles is
    // not either of their continuations, so neither is written over.
    if let Some((_, _, first)) = survey.valid.first()
        && let Some((_, _, other)) = survey
            .valid
            .iter()
            .find(|(_, _, held)| held.profile_id != first.profile_id)
    {
        return Err(CommitError::Recover(RecoverError::ProfileMismatch {
            first: first.profile_id,
            second: other.profile_id,
        }));
    }
    if let Some(index) = survey.best() {
        let stored = survey.valid[index].2.profile_id;
        if stored != document.profile_id {
            return Err(CommitError::WrongProfile {
                stored,
                offered: document.profile_id,
            });
        }
    }
    check_offer(&survey, document)?;
    run_phases(storage, &bytes, survey, &|held| {
        decode(held).map(|decoded| (decoded.revision, decoded))
    })?;
    Ok(())
}

/// Checks an offer against what a slot already holds: every valid file must
/// describe the same subject, and the offer's revision must be above the
/// highest stored one.
///
/// This is the second half of the commit rule, shared by every document kind
/// that has a slot, so a registry commit refuses a stale or foreign slot
/// exactly as a profile commit refuses a stale or foreign profile.
pub fn check_offer<T: SlotIdentity>(
    survey: &Survey<T>,
    offer: &impl SlotIdentity,
) -> Result<(), CommitError> {
    if let Some(first) = survey.valid.first() {
        for (file, _, other) in &survey.valid {
            if other.identity() != first.2.identity() {
                return Err(CommitError::SlotIdentityMismatch {
                    file: *file,
                    stored: first.2.identity(),
                    offered: other.identity(),
                });
            }
        }
    }
    if let Some((_, stored, _)) = survey
        .valid
        .iter()
        .find(|(_, revision, _)| *revision >= offer.revision())
    {
        return Err(CommitError::RevisionConflict {
            stored: *stored,
            offered: offer.revision(),
        });
    }
    Ok(())
}

/// A persisted document that names its own subject and carries a revision, so
/// the shared commit and selection rules apply to it.
pub trait SlotIdentity {
    /// A stable text identity of the subject this document describes. Two
    /// files of one slot that disagree on it are not one another's successor.
    fn identity(&self) -> String;
    /// The revision this document declares.
    fn revision(&self) -> Revision;
}

impl SlotIdentity for ProfileDocument {
    fn identity(&self) -> String {
        self.profile_id.to_string()
    }

    fn revision(&self) -> Revision {
        self.revision
    }
}
