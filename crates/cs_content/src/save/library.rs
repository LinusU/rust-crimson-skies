//! The production profile path: a population's profiles on a real filesystem.
//!
//! This is where the pieces meet. [`ProfileLibrary`] owns one population's
//! directory — its [registry](super::fs::Registry) and one slot directory per
//! profile — and is the only thing that writes a profile id, a profile save or
//! the active pointer. Every write goes through the five phases in
//! [`super::store`], so a kill at any point leaves a whole revision that
//! [`ProfileLibrary::load`] can find, and a kill that damages the newest file
//! falls back to the backup and says so in text the caller can display.
//!
//! Three properties the contract names are enforced here rather than left to a
//! consumer:
//!
//! * **Ids are persistent identities.** An id comes from the registry's
//!   high-water mark, which is persisted with its own backup. Deleting a
//!   profile never lowers the mark, so an old id can never come to mean a
//!   different pilot — including across a restart, which is the case a
//!   list-index identity fails.
//! * **Populations are separate.** The library is one [`ProfileKind`]'s
//!   directory, and a registry belonging to another kind is refused rather
//!   than adopted or overwritten, so a synthetic, modded or evidence session
//!   cannot reach a production profile.
//! * **Recovery is visible.** [`LoadedProfile`] carries the warnings as text
//!   ([`RecoveryWarning`]'s `Display`), so a fallback is something a caller
//!   shows rather than something it logs.
//!
//! Choosing the user-data base directory is F61 and left to the consumer;
//! this module is handed one and never chooses it.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, RegistryError, Revision};

use super::fs::{
    DirStorage, LoadedRegistry, REGISTRY_PREFIX, Registry, SlotError, commit_registry,
    load_registry, profile_slot, recover_profile, registry_slot,
};
use super::store::{
    CommitError, PROFILE_PREFIX, RecoverError, RecoveryWarning, SaveFile, commit, recover,
};

/// The directory name of one profile's slot.
///
/// Derived from the numeric [`ProfileId`] only. A display name never reaches a
/// path, so renaming a pilot cannot move their save and a name cannot be
/// spelled to traverse out of the base.
pub fn slot_name(id: ProfileId) -> String {
    format!("{PROFILE_PREFIX}-{id}")
}

/// The directory name a deleted profile's slot is kept under.
///
/// A retired slot is not removed, only renamed. That is what makes the id
/// high-water mark recoverable from the filesystem: an id that was issued is
/// still named on disk, live or retired, so a registry that regressed — the
/// newest one damaged and the fallback one an allocation behind — cannot cause
/// that id to be issued a second time. The retired files are kept rather than
/// destroyed for the same reason a damaged save is kept: a player may want
/// them, and no code should be the only thing that can still read them.
pub fn retired_name(id: ProfileId) -> String {
    format!("{RETIRED_PREFIX}-{id}")
}

/// The prefix of a retired slot's directory.
pub const RETIRED_PREFIX: &str = "retired";

/// One profile id the directory names, live or retired.
///
/// The name is the prefix, a `-` and the decimal id, and nothing else: a name
/// that does not parse whole is not an id, so it can never widen what a
/// directory is taken to prove.
fn id_of(name: &str, prefix: &str) -> Option<ProfileId> {
    ProfileId::new(name.strip_prefix(prefix)?.strip_prefix('-')?.parse().ok()?)
}

/// Every profile id the population's directory names, live slots and retired
/// slots alike, ascending.
fn observed_ids(base: &Path) -> Result<Vec<ProfileId>, LibraryError> {
    let mut ids = Vec::new();
    let entries = match std::fs::read_dir(base) {
        Ok(entries) => entries,
        // A population directory that does not exist yet holds no ids.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(error) => {
            return Err(LibraryError::Slot(SlotError::Io {
                path: base.to_path_buf(),
                reason: error.to_string(),
            }));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            LibraryError::Slot(SlotError::Io {
                path: base.to_path_buf(),
                reason: error.to_string(),
            })
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = id_of(&name, PROFILE_PREFIX).or_else(|| id_of(&name, RETIRED_PREFIX)) {
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Whether `path` is a slot directory this library could have written: a plain
/// directory, the same test [`DirStorage::create`] and [`retire_slot`] apply.
///
/// A symbolic link, a regular file or anything else is not a profile slot, so
/// it is never adopted as a live profile — a live profile the library cannot
/// write is worse than one it does not claim. The id it names still raises the
/// high-water mark, so a link is never a way to have an id issued again.
fn is_slot_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Whether `path` holds any of the three files of a save slot.
///
/// An empty slot directory is not a profile: a `create` killed between making
/// the directory and writing its first revision leaves exactly that, and
/// adopting it would offer the player a profile with no state to load.
fn has_save_files(path: &Path) -> bool {
    SaveFile::ALL
        .iter()
        .any(|file| std::fs::metadata(path.join(file.prefixed_name(PROFILE_PREFIX))).is_ok())
}

/// Why a library operation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryError {
    /// A directory or file could not be used as a slot.
    Slot(SlotError),
    /// The registry refused the operation.
    Registry(RegistryError),
    /// A save could not be committed.
    Commit(CommitError),
    /// A slot could not be read.
    Recover(RecoverError),
    /// A profile document does not belong to this population.
    ForeignDocument {
        id: ProfileId,
        stored: ProfileKind,
        offered: ProfileKind,
    },
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Slot(error) => write!(f, "{error}"),
            Self::Registry(error) => write!(f, "{error}"),
            Self::Commit(error) => write!(f, "{error}"),
            Self::Recover(error) => write!(f, "{error}"),
            Self::ForeignDocument {
                id,
                stored,
                offered,
            } => write!(
                f,
                "profile {id} is a {} profile, not {}",
                stored.label(),
                offered.label()
            ),
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<SlotError> for LibraryError {
    fn from(error: SlotError) -> Self {
        Self::Slot(error)
    }
}

impl From<RegistryError> for LibraryError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<CommitError> for LibraryError {
    fn from(error: CommitError) -> Self {
        Self::Commit(error)
    }
}

impl From<RecoverError> for LibraryError {
    fn from(error: RecoverError) -> Self {
        Self::Recover(error)
    }
}

/// One profile as the library found it, with every diagnostic it produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedProfile {
    pub id: ProfileId,
    /// The newest whole revision, or `None` for a slot that was never written.
    pub document: Option<ProfileDocument>,
    /// Which of the slot's files it came from.
    pub source: Option<super::store::SaveFile>,
    /// The diagnostics, as text a caller can show. Empty is the good case.
    pub warnings: Vec<String>,
}

impl LoadedProfile {
    /// Whether a whole revision was found.
    pub const fn is_present(&self) -> bool {
        self.document.is_some()
    }
}

/// Something opening a library had to reconcile, as text a caller can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryNotice {
    /// The registry named fewer profiles than the directory holds, or a lower
    /// high-water mark than an id already issued. The slots on disk are the
    /// authority for what exists, so the mark was raised and the unlisted live
    /// slots were adopted — otherwise a damaged newest registry would let a
    /// deleted id be issued again.
    Reconciled {
        registry_high_water: u64,
        adopted: Vec<ProfileId>,
    },
}

impl fmt::Display for LibraryNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reconciled {
                registry_high_water,
                adopted,
            } if adopted.is_empty() => write!(
                f,
                "the registry recorded a high-water mark of {registry_high_water}; \
                 the profile directory names a higher id, so the mark was raised \
                 rather than lowered"
            ),
            Self::Reconciled {
                registry_high_water,
                adopted,
            } => write!(
                f,
                "the registry recorded a high-water mark of {registry_high_water} \
                 and did not list {}; the profile directories are the authority, \
                 so the mark was raised and {} adopted",
                adopted.len(),
                describe_ids(adopted)
            ),
        }
    }
}

/// A list of ids as text a caller can show, without a trailing separator.
fn describe_ids(ids: &[ProfileId]) -> String {
    ids.iter()
        .map(ProfileId::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What opening a library recovered, so a caller can show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryStatus {
    /// Diagnostics from the registry slot.
    pub registry_warnings: Vec<String>,
    /// What the directory said that the registry did not.
    pub notices: Vec<LibraryNotice>,
    /// Whether a registry file was actually read: a population with none has
    /// a high-water mark of zero, which is a fact about it and not a failure.
    pub registry_persisted: bool,
    /// The high-water mark now in force.
    pub high_water: u64,
    /// The live ids, in allocation order.
    pub live: Vec<ProfileId>,
    /// The active pointer, if one is live.
    pub active: Option<ProfileId>,
}

impl LibraryStatus {
    /// Every diagnostic from opening this population, as text a caller can show:
    /// the registry slot's own warnings *and* the reconciliations this open had
    /// to perform.
    ///
    /// Both kinds belong here. A notice is not a warning — nothing failed — but
    /// it is a decision this build made about the player's profile set (which
    /// slots it adopted, which mark it raised), and a caller that cannot see it
    /// is looking at a population whose shape it cannot account for.
    pub fn warning_lines(&self) -> Vec<String> {
        let mut lines = self.registry_warnings.clone();
        lines.extend(self.notices.iter().map(ToString::to_string));
        lines
    }
}

/// One population's profiles on a real filesystem.
///
/// Created by [`ProfileLibrary::open`], which creates the directory and
/// recovers the registry. A library owns its population's files: two live
/// libraries over one directory would each believe they were alone, so the
/// consumer is expected to hold one (F48-C is the consumer that wires this into
/// the runtime and decides the teardown).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileLibrary {
    base: PathBuf,
    kind: ProfileKind,
    registry: Registry,
    status: LibraryStatus,
}

impl ProfileLibrary {
    /// Opens one population's library in `base`, creating the directory if it is
    /// absent and recovering the registry slot.
    ///
    /// The registry is the primary record, but it is not the only evidence: a
    /// slot directory names the id it belongs to, and a deleted profile's slot
    /// is *renamed* to a retired slot rather than removed. So the id high-water
    /// mark in force is the highest of the registry's mark and every id the
    /// directory names, and any live slot the registry did not list is adopted
    /// into the live set. That is what makes a regressed registry safe: a
    /// damaged newest registry falls back to an older one, and the ids that
    /// older record had not yet seen are still on disk and are therefore still
    /// not reissued. The reconciliation is reported in
    /// [`LibraryStatus::notices`], never applied silently.
    ///
    /// A registry that does not decode *and* leaves no readable slot is an
    /// error rather than an empty library: a high-water mark that cannot be
    /// read from anywhere must not be treated as zero.
    pub fn open(base: impl Into<PathBuf>, kind: ProfileKind) -> Result<Self, LibraryError> {
        let base = base.into();
        let loaded = match load_registry(&base, kind) {
            Ok(loaded) => loaded,
            Err(error) => {
                // The registry is unreadable. The slots may still be, and they
                // carry their own ids, so a population that exists is recovered
                // from them rather than declared empty.
                let observed = observed_ids(&base)?;
                if observed.is_empty() {
                    return Err(error.into());
                }
                return Self::from_observed(base, kind, &error.to_string(), observed);
            }
        };
        Self::from_loaded(base, kind, loaded)
    }

    fn from_loaded(
        base: PathBuf,
        kind: ProfileKind,
        loaded: LoadedRegistry,
    ) -> Result<Self, LibraryError> {
        let registry_warnings = loaded.warning_lines();
        let (registry, notices) = Self::reconcile(base.as_path(), kind, &loaded.registry)?;
        let status = LibraryStatus {
            registry_warnings,
            notices,
            registry_persisted: loaded.was_persisted,
            high_water: registry.high_water(),
            live: registry.live().to_vec(),
            active: registry.active(),
        };
        Ok(Self {
            base,
            kind,
            registry,
            status,
        })
    }

    /// A library rebuilt from the ids the directory names, for a registry that
    /// could not be read at all.
    ///
    /// Every named id raises the mark, live or retired, because an id that was
    /// issued must not be issued again. Only a *live* slot is a profile: a
    /// retired slot is a deleted profile's kept files, not a pilot, so it is
    /// not adopted into the live set. The active pointer is gone with the
    /// registry that held it, and a caller chooses one explicitly.
    fn from_observed(
        base: PathBuf,
        kind: ProfileKind,
        reason: &str,
        observed: Vec<ProfileId>,
    ) -> Result<Self, LibraryError> {
        let high_water = observed.iter().map(|id| id.get()).max().unwrap_or(0);
        let live: Vec<ProfileId> = observed
            .iter()
            .copied()
            .filter(|id| is_slot_directory(&base.join(slot_name(*id))))
            .filter(|id| has_save_files(&base.join(slot_name(*id))))
            .collect();
        // No readable registry file, so the next write offers revision 1: there
        // is nothing on disk for that to conflict with, and the mark carried
        // from the directory is what stops an issued id returning.
        let registry = Registry::rebuilt(Revision(0), kind, high_water, live, None)?;
        let status = LibraryStatus {
            registry_warnings: vec![reason.to_owned()],
            notices: vec![LibraryNotice::Reconciled {
                registry_high_water: 0,
                adopted: registry.live().to_vec(),
            }],
            registry_persisted: false,
            high_water,
            live: registry.live().to_vec(),
            active: None,
        };
        Ok(Self {
            base,
            kind,
            registry,
            status,
        })
    }

    /// Raises the mark to every id the directory names and adopts live slots
    /// the registry did not list.
    ///
    /// The record's own revision is carried over, so the next registry write is
    /// a continuation of the file that is in force rather than a second attempt
    /// at a revision already used.
    fn reconcile(
        base: &Path,
        kind: ProfileKind,
        registry: &Registry,
    ) -> Result<(Registry, Vec<LibraryNotice>), LibraryError> {
        let observed = observed_ids(base)?;
        if observed.is_empty() {
            return Ok((registry.clone(), Vec::new()));
        }
        let high_water = observed
            .iter()
            .map(|id| id.get())
            .max()
            .unwrap_or(registry.high_water())
            .max(registry.high_water());
        // A live slot the registry did not list is a profile whose save landed
        // but whose registry write did not; adopting it is the only reading
        // under which the directory and the record agree. A slot that is not a
        // plain directory, or that holds no save file, is not a profile whose
        // save landed: its id still raises the mark, but it is not offered as a
        // pilot with no state behind it.
        let adopted: Vec<ProfileId> = observed
            .iter()
            .copied()
            .filter(|id| !registry.live().contains(id))
            .filter(|id| {
                let directory = base.join(slot_name(*id));
                is_slot_directory(&directory) && has_save_files(&directory)
            })
            .collect();
        if high_water == registry.high_water() && adopted.is_empty() {
            return Ok((registry.clone(), Vec::new()));
        }
        let mut live = registry.live().to_vec();
        live.extend(adopted.iter().copied());
        let reconciled = Registry::rebuilt(
            registry.revision(),
            kind,
            high_water,
            live,
            registry.active(),
        )?;
        Ok((
            reconciled,
            vec![LibraryNotice::Reconciled {
                registry_high_water: registry.high_water(),
                adopted,
            }],
        ))
    }

    /// The population this library owns.
    pub const fn kind(&self) -> ProfileKind {
        self.kind
    }

    /// The base directory this library was opened at.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// What opening the library found, including the high-water mark in force.
    pub const fn status(&self) -> &LibraryStatus {
        &self.status
    }

    /// The live ids, in allocation order.
    pub fn live(&self) -> &[ProfileId] {
        self.registry.live()
    }

    /// The active profile, if one is live.
    pub const fn active(&self) -> Option<ProfileId> {
        self.registry.active()
    }

    /// One profile's slot directory.
    pub fn slot_dir(&self, id: ProfileId) -> PathBuf {
        self.base.join(slot_name(id))
    }

    /// Allocates a new profile id and writes the first revision of a profile
    /// that carries it.
    ///
    /// The id is taken from the persisted high-water mark, so it is above
    /// every id this population has ever issued, and the profile document it
    /// is written into states the same id: the id is the identity, not a label
    /// derived from the document afterwards.
    pub fn create(
        &mut self,
        document: ProfileDocument,
    ) -> Result<(ProfileId, ProfileDocument), LibraryError> {
        if document.kind != self.kind {
            return Err(LibraryError::ForeignDocument {
                id: document.profile_id,
                stored: self.kind,
                offered: document.kind,
            });
        }
        let (id, registry) = self.registry.allocate()?;
        let document = ProfileDocument {
            profile_id: id,
            revision: Revision(1),
            ..document
        };
        // The save lands first: a profile that exists on disk but is missing
        // from the registry is recoverable (the next open sees an unlisted
        // slot), while a registry that names a profile with no save is a
        // dangling live id.
        self.write_document(&document)?;
        let written = self.persist(registry)?;
        self.status.live = written.live().to_vec();
        self.status.active = written.active();
        self.status.high_water = written.high_water();
        Ok((id, document))
    }

    /// Reads one profile's newest whole revision and every diagnostic.
    pub fn load(&self, id: ProfileId) -> Result<LoadedProfile, LibraryError> {
        let found = recover_profile(&self.slot_dir(id))?;
        Ok(match found {
            Some((recovery, warnings)) => LoadedProfile {
                id,
                document: Some(recovery.document),
                source: Some(recovery.source),
                warnings,
            },
            None => LoadedProfile {
                id,
                document: None,
                source: None,
                warnings: Vec::new(),
            },
        })
    }

    /// Commits a new revision of a live profile.
    ///
    /// The revision must be above the stored one, and the document must belong
    /// to this population, so a stale write or a foreign document is refused
    /// rather than installed. Nothing is written when the document fails to
    /// encode.
    pub fn save(&mut self, document: &ProfileDocument) -> Result<(), LibraryError> {
        if document.kind != self.kind {
            return Err(LibraryError::ForeignDocument {
                id: document.profile_id,
                stored: self.kind,
                offered: document.kind,
            });
        }
        if !self.registry.is_live(document.profile_id) {
            return Err(LibraryError::Registry(RegistryError::UnknownProfile(
                document.profile_id,
            )));
        }
        self.write_document(document)
    }

    /// Deletes a profile: the registry stops listing it and its slot is
    /// renamed to a retired slot.
    ///
    /// The high-water mark does not move, so the id is never issued again.
    /// The slot is renamed rather than removed for the same reason: an id that
    /// was issued stays named on disk, so a registry that regresses to an older
    /// revision still cannot cause that id to be reissued (see
    /// [`ProfileLibrary::open`]). The rename happens after the registry write,
    /// so a kill in between leaves a live-looking directory that reconciliation
    /// then adopts back — a lost delete, not a lost profile. A slot path that is
    /// not a plain directory this library could have written is never touched.
    pub fn delete(&mut self, id: ProfileId) -> Result<(), LibraryError> {
        let registry = self.registry.delete(id)?;
        let written = self.persist(registry)?;
        self.status.live = written.live().to_vec();
        self.status.active = written.active();
        self.status.high_water = written.high_water();
        retire_slot(&self.slot_dir(id), &self.base.join(retired_name(id)))?;
        Ok(())
    }

    /// Points the active pointer at a live profile and persists it.
    pub fn set_active(&mut self, id: ProfileId) -> Result<(), LibraryError> {
        let registry = self.registry.set_active(id)?;
        let written = self.persist(registry)?;
        self.status.active = written.active();
        self.status.live = written.live().to_vec();
        Ok(())
    }

    /// Writes one profile document into its slot, creating the slot directory.
    fn write_document(&self, document: &ProfileDocument) -> Result<(), LibraryError> {
        let directory = self.slot_dir(document.profile_id);
        let mut slot = DirStorage::create(directory, PROFILE_PREFIX)?;
        commit(&mut slot, document)?;
        Ok(())
    }

    /// The typed recovery diagnostics of one profile's slot, for a caller that
    /// keeps the structured form as well as the text in [`LoadedProfile`].
    pub fn slot_warnings(&self, id: ProfileId) -> Result<Vec<RecoveryWarning>, LibraryError> {
        Ok(recover(&profile_slot(&self.slot_dir(id)))?
            .map_or_else(Vec::new, |found| found.warnings))
    }

    /// Commits a registry revision through the same five phases as a save.
    fn persist(&mut self, registry: Registry) -> Result<Registry, LibraryError> {
        // The population directory is created on the first write, and
        // `DirStorage::create` refuses a path that is not a plain directory.
        // The registry's own slot is the one living in that directory, so that
        // is the prefix the check is made with.
        DirStorage::create(&self.base, REGISTRY_PREFIX)?;
        let mut slot = registry_slot(&self.base);
        let written = commit_registry(&mut slot, &registry)?;
        self.registry = written.clone();
        Ok(written)
    }
}

/// Retires one profile slot: renames its directory so the id stays named on
/// disk, refusing anything that is not a plain directory this library could
/// have written.
fn retire_slot(from: &Path, to: &Path) -> Result<(), LibraryError> {
    let refuse = |path: &Path| LibraryError::Slot(SlotError::NotADirectory(path.to_path_buf()));
    match std::fs::symlink_metadata(from) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(refuse(from))
        }
        Ok(_) => {
            // A retired slot that is already there is the state being asked
            // for; a second attempt must not destroy the first one's files.
            if std::fs::symlink_metadata(to).is_ok() {
                return Err(refuse(to));
            }
            std::fs::rename(from, to).map_err(|error| {
                LibraryError::Slot(SlotError::Io {
                    path: from.to_path_buf(),
                    reason: error.to_string(),
                })
            })
        }
        // A slot that is already gone is the state the caller asked for.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LibraryError::Slot(SlotError::Io {
            path: from.to_path_buf(),
            reason: error.to_string(),
        })),
    }
}

/// Reads a profile slot without opening a library, for a caller that has a
/// path and nothing else.
pub fn load_profile_slot(directory: &Path) -> Result<Option<ProfileDocument>, RecoverError> {
    Ok(recover(&profile_slot(directory))?.map(|found| found.document))
}
