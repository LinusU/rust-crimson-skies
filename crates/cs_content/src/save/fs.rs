//! The real filesystem behind [`SaveStorage`], and the profile registry slot.
//!
//! [`DirStorage`] maps each [`SavePhase`] of `store::commit` onto a real file
//! operation in one directory:
//!
//! | Phase | Filesystem operation |
//! | --- | --- |
//! | [`SavePhase::WriteTemp`] | create + `write_all` + `flush` of `prefix.tmp` |
//! | [`SavePhase::SyncTemp`] | `File::sync_all` of the temp file |
//! | [`SavePhase::RotateBackup`] | `rename(prefix.sav, prefix.bak)` |
//! | [`SavePhase::InstallCurrent`] | `rename(prefix.tmp, prefix.sav)` |
//! | [`SavePhase::SyncDir`] | `File::sync_all` of the directory |
//!
//! Both renames are within one directory, so each is atomic on every platform
//! `std::fs::rename` supports. What that means for the *replacement* of an
//! existing file is platform-specific, so [`replace`] is the single place that
//! knows it and [`platform_note`] reports what this build actually did.
//!
//! The registry ([`Registry`]) is the same three-file slot for a different
//! document: the live profile ids, the active pointer and the id high-water
//! mark, with the same commit and selection rules. It lives here rather than in
//! `store` because it is persistence state, not a save document.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use cs_types::profile::{ProfileId, ProfileKind, ProfileRegistry, RegistryError, Revision};

use super::codec::{
    self, CHECKSUM_KEY, DecodeError, MAX_SAVE_BYTES, check_line_count, sealed_body,
};
use super::store::{
    CommitError, PROFILE_PREFIX, RecoverError, Recovery, RecoveryWarning, SaveFile, SaveStorage,
    SlotIdentity, StorageError, check_offer, recover, run_phases, survey,
};

/// The file name prefix of the registry slot.
pub const REGISTRY_PREFIX: &str = "registry";

/// The magic word in a registry document's header line, so a registry is never
/// mistaken for a save (or the other way round) by either decoder.
const REGISTRY_MAGIC: &str = "CSREG";

/// The header line a registry document starts with: the magic word and the
/// schema version, which the decoder re-reads and checks rather than trusting.
const REGISTRY_HEADER: &str = concat!("CSREG", " 1.0\n");

/// Which rename call this build's replacement is on this platform.
///
/// The contract refuses to assume POSIX rename behavior on Windows, so the
/// difference is reported rather than described. `std::fs::rename` maps to
/// `rename(2)` on unix and to `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`
/// on Windows: both replace an existing destination, but they differ in what
/// happens to an open or read-only destination, and that difference is a
/// platform property this build states rather than assumes. Which one behaves
/// correctly under a real crash on each supported platform is F48-D's
/// measurement, not this module's claim.
pub const fn replacement_semantics() -> Replacement {
    if cfg!(windows) {
        Replacement::MoveFileEx
    } else {
        Replacement::Rename
    }
}

/// How this build replaces a file that already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replacement {
    /// `rename(2)`: the destination entry is replaced atomically; the replaced
    /// inode is unlinked.
    Rename,
    /// `MoveFileExW`: the destination is replaced, and a read-only destination
    /// is a failure rather than a silent replacement.
    MoveFileEx,
}

impl fmt::Display for Replacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rename => write!(f, "rename(2)"),
            Self::MoveFileEx => write!(f, "MoveFileExW"),
        }
    }
}

/// Whether this platform can sync a directory handle.
///
/// A directory `fsync` is what makes a rename durable; where it is unavailable
/// the phase is a documented no-op instead of a silent one.
pub const fn directory_sync_supported() -> bool {
    cfg!(unix)
}

/// One line about the platform behavior of the write path, for a diagnostics
/// surface that reports what this build did rather than what it assumed.
pub fn platform_note() -> String {
    format!(
        "save writes use {} replacement; directory sync is {}",
        replacement_semantics(),
        if directory_sync_supported() {
            "supported"
        } else {
            "a no-op on this platform"
        }
    )
}

/// Replaces `destination` with `source` atomically.
fn replace(source: &Path, destination: &Path) -> Result<(), StorageError> {
    fs::rename(source, destination).map_err(|error| {
        StorageError::Io(format!(
            "{} -> {}: {error}",
            source.display(),
            destination.display()
        ))
    })
}

/// Syncs a directory entry so a rename inside it survives a power loss.
#[cfg(unix)]
fn sync_dir(directory: &Path) -> Result<(), StorageError> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|error| {
            StorageError::Io(format!(
                "cannot sync directory {}: {error}",
                directory.display()
            ))
        })
}

/// A directory that cannot be opened as a directory handle is reported, not
/// assumed away: the caller decides what an unsynced rename means.
#[cfg(not(unix))]
fn sync_dir(_directory: &Path) -> Result<(), StorageError> {
    Ok(())
}

/// Why a real directory could not be used as a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// The path exists and is not a plain directory, so a save would write
    /// beside it or through a link rather than inside it.
    NotADirectory(PathBuf),
    Io {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory(path) => {
                write!(f, "{} is not a directory", path.display())
            }
            Self::Io { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for SlotError {}

impl From<SlotError> for StorageError {
    fn from(error: SlotError) -> Self {
        Self::Io(error.to_string())
    }
}

/// One real directory holding the three files of a save slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirStorage {
    directory: PathBuf,
    prefix: String,
}

impl DirStorage {
    /// A slot in an existing directory.
    pub fn new(directory: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            prefix: prefix.into(),
        }
    }

    /// A slot in `directory`, creating the directory if it is absent.
    ///
    /// A path that already exists and is not a plain directory is refused, so a
    /// save never follows a symbolic link or writes into a regular file.
    pub fn create(
        directory: impl AsRef<Path>,
        prefix: impl Into<String>,
    ) -> Result<Self, SlotError> {
        let directory = directory.as_ref();
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(SlotError::NotADirectory(directory.to_path_buf()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(directory).map_err(|error| SlotError::Io {
                    path: directory.to_path_buf(),
                    reason: error.to_string(),
                })?;
            }
            Err(error) => {
                return Err(SlotError::Io {
                    path: directory.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        }
        Ok(Self::new(directory, prefix))
    }

    /// The slot's directory.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The path of one of the slot's files.
    pub fn path(&self, file: SaveFile) -> PathBuf {
        self.directory.join(file.prefixed_name(&self.prefix))
    }
}

impl SaveStorage for DirStorage {
    fn read(&self, file: SaveFile) -> Result<Option<Vec<u8>>, StorageError> {
        match fs::read(self.path(file)) {
            Ok(bytes) => {
                if bytes.len() > MAX_SAVE_BYTES {
                    // Refused here rather than handed on: a caller that reads
                    // and ignores a decode refusal would then treat the file as
                    // an ignorable diagnostic and install a new revision over
                    // bytes it never looked at.
                    return Err(StorageError::Io(format!(
                        "{} is {} bytes, over the {MAX_SAVE_BYTES} a save may be",
                        self.path(file).display(),
                        bytes.len()
                    )));
                }
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StorageError::Io(format!(
                "{}: {error}",
                self.path(file).display()
            ))),
        }
    }

    fn write_temp(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let path = self.path(SaveFile::Temp);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|error| StorageError::Io(format!("{}: {error}", path.display())))?;
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .map_err(|error| StorageError::Io(format!("{}: {error}", path.display())))
    }

    fn sync_temp(&mut self) -> Result<(), StorageError> {
        let path = self.path(SaveFile::Temp);
        // Reopened rather than kept open: the temp file must be durable through
        // its own descriptor, exactly as it would be after a crash.
        File::open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| StorageError::Io(format!("{}: {error}", path.display())))
    }

    fn rotate_backup(&mut self) -> Result<(), StorageError> {
        replace(&self.path(SaveFile::Current), &self.path(SaveFile::Backup))
    }

    fn install_current(&mut self) -> Result<(), StorageError> {
        replace(&self.path(SaveFile::Temp), &self.path(SaveFile::Current))
    }

    fn sync_dir(&mut self) -> Result<(), StorageError> {
        sync_dir(&self.directory)
    }
}

/// A profile slot on the real filesystem.
pub fn profile_slot(directory: &Path) -> DirStorage {
    DirStorage::new(directory, PROFILE_PREFIX)
}

/// Reads one profile slot from a real directory, with the warnings recovery
/// produced. `Ok(None)` is a fresh, unwritten slot.
pub fn recover_profile(directory: &Path) -> Result<Option<(Recovery, Vec<String>)>, RecoverError> {
    Ok(recover(&profile_slot(directory))?.map(|found| {
        let lines = found.warning_lines();
        (found, lines)
    }))
}

/// The persisted registry: live ids, the active pointer and the id high-water
/// mark.
///
/// This is the file that makes a profile id a persistent identity rather than a
/// list index (F48 non-negotiable 3). It is written through the same five
/// phases as a save, so a torn registry write cannot lower the high-water mark
/// to a value that reissues a live or deleted id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    revision: Revision,
    kind: ProfileKind,
    high_water: u64,
    live: Vec<ProfileId>,
    active: Option<ProfileId>,
}

impl Registry {
    /// The registry a fresh, empty population starts from.
    pub fn empty(kind: ProfileKind) -> Self {
        Self {
            revision: Revision(0),
            kind,
            high_water: 0,
            live: Vec::new(),
            active: None,
        }
    }

    pub const fn kind(&self) -> ProfileKind {
        self.kind
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub fn high_water(&self) -> u64 {
        self.high_water
    }

    pub fn live(&self) -> &[ProfileId] {
        &self.live
    }

    pub const fn active(&self) -> Option<ProfileId> {
        self.active
    }

    /// The registry rebuilt from a decoded record, refusing an inconsistent
    /// set (a mark below a live id, a repeated id) and dropping a dangling
    /// active pointer rather than trusting it.
    fn from_record(record: RegistryRecord) -> Result<Self, RegistryError> {
        let parts = ProfileRegistry::from_parts(record.high_water, record.live, record.active)?;
        Ok(Self {
            revision: record.revision,
            kind: record.kind,
            high_water: parts.high_water(),
            live: parts.live().to_vec(),
            active: parts.active(),
        })
    }

    /// The revision this registry publishes as. A write publishes
    /// `revision + 1`, so the stored revision is the one that was written.
    fn advanced(&self) -> Result<Self, CommitError> {
        Ok(Self {
            revision: self.revision.next().ok_or(CommitError::RevisionExhausted)?,
            kind: self.kind,
            high_water: self.high_water,
            live: self.live.clone(),
            active: self.active,
        })
    }

    /// The registry after allocating a fresh id above the high-water mark, and
    /// the id that was issued.
    ///
    /// The id comes from the persisted mark, not from the live list, so
    /// deleting a profile never makes its id available again.
    pub fn allocate(&self) -> Result<(ProfileId, Self), RegistryError> {
        let mut parts =
            ProfileRegistry::from_parts(self.high_water, self.live.clone(), self.active)?;
        let id = parts.allocate()?;
        Ok((
            id,
            Self {
                revision: self.revision,
                kind: self.kind,
                high_water: parts.high_water(),
                live: parts.live().to_vec(),
                active: parts.active(),
            },
        ))
    }

    /// The registry after deleting a live profile. The high-water mark does not
    /// move, so the id is never issued again.
    pub fn delete(&self, id: ProfileId) -> Result<Self, RegistryError> {
        let mut parts =
            ProfileRegistry::from_parts(self.high_water, self.live.clone(), self.active)?;
        parts.delete(id)?;
        Ok(Self {
            revision: self.revision,
            kind: self.kind,
            high_water: parts.high_water(),
            live: parts.live().to_vec(),
            active: parts.active(),
        })
    }

    /// The registry after pointing the active pointer at a live profile.
    pub fn set_active(&self, id: ProfileId) -> Result<Self, RegistryError> {
        let mut parts =
            ProfileRegistry::from_parts(self.high_water, self.live.clone(), self.active)?;
        parts.set_active(id)?;
        Ok(Self {
            revision: self.revision,
            kind: self.kind,
            high_water: parts.high_water(),
            live: parts.live().to_vec(),
            active: parts.active(),
        })
    }

    /// Whether `id` is live in this population.
    pub fn is_live(&self, id: ProfileId) -> bool {
        self.live.contains(&id)
    }

    /// A registry whose parts were read off the filesystem rather than off a
    /// registry record: a slot directory names the id it belongs to, so the
    /// mark can be recovered without a trustworthy registry file.
    ///
    /// `revision` is the revision of the record the parts came from, or zero
    /// when there was none. Keeping it is what makes the next write a
    /// continuation of what is stored rather than a second attempt at an
    /// already-used revision.
    ///
    /// The parts are still checked by the same rule — a mark below a live id or
    /// a repeated id is refused — so this is not a way around the consistency
    /// requirement; it is the same construction with a mark that is known to
    /// cover the live ids because it is their maximum.
    pub fn rebuilt(
        revision: Revision,
        kind: ProfileKind,
        high_water: u64,
        live: Vec<ProfileId>,
        active: Option<ProfileId>,
    ) -> Self {
        // `rebuild_from_live` computes the maximum of the live ids, so the
        // parts are consistent by construction when the mark is that maximum.
        let mut parts = ProfileRegistry::rebuild_from_live(live, active);
        if high_water > parts.high_water() {
            // A mark above every live id is kept: a deleted id above the live
            // set is exactly what must never be issued again, so it is carried
            // rather than lowered.
            parts = ProfileRegistry::from_parts(high_water, parts.live().to_vec(), parts.active())
                .unwrap_or(
                    ProfileRegistry::from_parts(high_water, parts.live().to_vec(), None)
                        .expect("a mark above every live id is consistent"),
                );
        }
        Self {
            revision,
            kind,
            high_water: parts.high_water(),
            live: parts.live().to_vec(),
            active: parts.active(),
        }
    }
}

impl SlotIdentity for Registry {
    /// A registry slot belongs to exactly one population, so the population is
    /// its identity: a registry of another kind is not this slot's successor.
    fn identity(&self) -> String {
        self.kind.label().to_owned()
    }

    fn revision(&self) -> Revision {
        self.revision
    }
}

/// One decoded registry line set.
struct RegistryRecord {
    revision: Revision,
    kind: ProfileKind,
    high_water: u64,
    live: Vec<ProfileId>,
    active: Option<ProfileId>,
}

/// Encodes a registry as a checksummed line document, using the same framing
/// and bounds as a save (`sealed_body`, `check_line_count`).
///
/// ```text
/// CSREG 1.0
/// kind=<label>
/// revision=<n>
/// high_water=<n>
/// live=<id>            (repeated, in allocation order)
/// active=<id>          (only when a profile is active)
/// checksum=<16 hex>
/// ```
pub fn encode_registry(registry: &Registry) -> Vec<u8> {
    let mut text = String::new();
    text.push_str(REGISTRY_HEADER);
    text.push_str(&format!("kind={}\n", registry.kind.label()));
    text.push_str(&format!("revision={}\n", registry.revision.0));
    text.push_str(&format!("high_water={}\n", registry.high_water));
    for id in &registry.live {
        text.push_str(&format!("live={id}\n"));
    }
    if let Some(active) = registry.active {
        text.push_str(&format!("active={active}\n"));
    }
    let sum = codec::checksum(text.as_bytes());
    text.push_str(&format!("{CHECKSUM_KEY}{sum:016x}\n"));
    text.into_bytes()
}

/// Decodes a registry document. Never panics and never interprets a field
/// before its size and character bounds are checked.
pub fn decode_registry(bytes: &[u8]) -> Result<(Revision, Registry), DecodeError> {
    let (covered, _) = sealed_body(REGISTRY_MAGIC, bytes)?;
    check_line_count(covered)?;
    let mut revision = None;
    let mut kind = None;
    let mut high_water = None;
    let mut live: Vec<ProfileId> = Vec::new();
    let mut active: Option<Option<ProfileId>> = None;
    for (index, line) in covered.lines().enumerate() {
        let number = index + 2;
        let (key, value) = line.split_once('=').ok_or(DecodeError::Malformed {
            line: number,
            reason: "expected key=value",
        })?;
        match key {
            "revision" => {
                once(&mut revision, number)?;
                revision = Some(Revision(codec::parse_u64(number, value)?));
            }
            "kind" => {
                once(&mut kind, number)?;
                kind = Some(
                    ProfileKind::from_label(value).ok_or(DecodeError::Malformed {
                        line: number,
                        reason: "unknown profile kind",
                    })?,
                );
            }
            "high_water" => {
                once(&mut high_water, number)?;
                high_water = Some(codec::parse_u64(number, value)?);
            }
            "live" => {
                if live.len() >= cs_types::profile::MAX_LIST_ENTRIES {
                    return Err(DecodeError::TooManyLines);
                }
                live.push(codec::parse_profile_id(number, value)?);
            }
            "active" => {
                once(&mut active, number)?;
                active = Some(Some(codec::parse_profile_id(number, value)?));
            }
            _ => {
                return Err(DecodeError::Malformed {
                    line: number,
                    reason: "unknown registry field",
                });
            }
        }
    }
    let record = RegistryRecord {
        revision: revision.ok_or(DecodeError::MissingField("revision"))?,
        kind: kind.ok_or(DecodeError::MissingField("kind"))?,
        high_water: high_water.ok_or(DecodeError::MissingField("high_water"))?,
        live,
        active: active.unwrap_or(None),
    };
    // An inconsistent registry is a whole, intact file whose fields do not hold
    // together, so it is reported as such and the recovery rule above still
    // treats it as an ignorable file — never as a revision to build on.
    let registry = Registry::from_record(record).map_err(|error| {
        let reason = match error {
            RegistryError::HighWaterBelowLive { high_water, live } => {
                format!("high-water mark {high_water} is below live profile {live}")
            }
            RegistryError::DuplicateLive(id) => format!("profile {id} is listed twice"),
            RegistryError::UnknownProfile(id) => format!("profile {id} is not live"),
            RegistryError::Exhausted => "profile ids are exhausted".to_owned(),
        };
        DecodeError::Inconsistent {
            document: "the profile registry",
            reason,
        }
    })?;
    Ok((registry.revision, registry))
}

/// A single-valued field: a repeat is refused rather than last-write-wins, so
/// an appended field cannot silently change what a document says.
fn once<T>(slot: &mut Option<T>, line: usize) -> Result<(), DecodeError> {
    if slot.is_some() {
        return Err(DecodeError::Malformed {
            line,
            reason: "field repeated",
        });
    }
    Ok(())
}

/// The registry slot of one population's directory.
pub fn registry_dir(base: &Path, kind: ProfileKind) -> PathBuf {
    base.join(kind.label())
}

/// A registry slot in a real directory.
pub fn registry_slot(directory: &Path) -> DirStorage {
    DirStorage::new(directory, REGISTRY_PREFIX)
}

/// What opening a registry slot found, with every diagnostic it produced.
///
/// A caller that owns a display surface shows `warnings`; nothing about a
/// fallback is only available in a log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedRegistry {
    /// The registry in force: the highest whole valid one, or the empty
    /// registry of a population that has never been written.
    pub registry: Registry,
    /// The files that did not decode, with why, and the fallback notice when
    /// the state came from somewhere other than the current file.
    pub warnings: Vec<RecoveryWarning>,
    /// Whether any registry file was read at all. A population with no
    /// registry file has a high-water mark of zero, and that is a real fact
    /// about it rather than a failure.
    pub was_persisted: bool,
}

impl LoadedRegistry {
    /// The warnings as text.
    pub fn warning_lines(&self) -> Vec<String> {
        self.warnings.iter().map(ToString::to_string).collect()
    }
}

/// Reads a registry slot from a real directory.
///
/// The same selection rule as a profile save: each whole file is decoded, the
/// valid one with the highest revision wins, an unreadable schema refuses the
/// whole slot rather than being shadowed, and no two files are combined.
pub fn load_registry(directory: &Path, kind: ProfileKind) -> Result<LoadedRegistry, RecoverError> {
    let slot = registry_slot(directory);
    let mut survey = survey(&slot, decode_registry)?;
    let Some(best) = survey.best() else {
        return if survey.corrupt.is_empty() {
            Ok(LoadedRegistry {
                registry: Registry::empty(kind),
                warnings: Vec::new(),
                was_persisted: false,
            })
        } else {
            Err(RecoverError::NoValidSave {
                diagnostics: survey.corrupt,
            })
        };
    };
    let (source, _, registry) = survey.valid.swap_remove(best);
    if registry.kind != kind {
        // A registry of another population is not this directory's registry.
        // It is refused rather than adopted, and never overwritten: the caller
        // is told, and the bytes stay.
        return Err(RecoverError::RegistryKindMismatch {
            stored: registry.kind,
            offered: kind,
        });
    }
    let mut warnings: Vec<RecoveryWarning> = survey
        .corrupt
        .into_iter()
        .map(|(file, error)| RecoveryWarning::Corrupt { file, error })
        .collect();
    if source != SaveFile::Current {
        warnings.push(RecoveryWarning::UsedFallback { source });
    }
    Ok(LoadedRegistry {
        registry,
        warnings,
        was_persisted: true,
    })
}

/// Writes the next revision of `registry` into its slot, through the same five
/// phases a profile save uses.
///
/// A registry whose revision is not above the stored one is refused
/// ([`CommitError::RevisionConflict`]), which is what stops a lost update from
/// lowering the high-water mark and reissuing a deleted id.
pub fn commit_registry(
    slot: &mut dyn SaveStorage,
    registry: &Registry,
) -> Result<Registry, CommitError> {
    let offer = registry.advanced()?;
    let survey = survey(slot, decode_registry).map_err(CommitError::Recover)?;
    check_offer(&survey, &offer)?;
    run_phases(slot, &encode_registry(&offer), survey, &decode_registry)?;
    Ok(offer)
}

/// Writes `registry` into the real directory `directory`, creating it if needed.
pub fn save_registry(directory: &Path, registry: &Registry) -> Result<Registry, SlotError> {
    let mut slot = DirStorage::create(directory, REGISTRY_PREFIX)?;
    commit_registry(&mut slot, registry).map_err(|error| SlotError::Io {
        path: directory.to_path_buf(),
        reason: error.to_string(),
    })
}

/// One line about what a recovery did, for a diagnostics surface.
pub fn recovery_line(warnings: &[RecoveryWarning]) -> String {
    if warnings.is_empty() {
        return "no recovery was needed".to_owned();
    }
    warnings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
