//! Acceptance scenarios F48-B: atomic persistence, persistent ids and recovery
//! on the real filesystem. Task test prefix: `accept_f48_b_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`, stage
//! `### F48-B`; contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Persistence"). Every value is newly authored synthetic data written under
//! the system temporary directory: no test touches a real user profile
//! directory, `$CS_GAME_DIR` or any original data, and every tree is removed
//! again when the test finishes.
//!
//! These tests call the production path — `cs_content::save::fs::DirStorage`
//! through `store::commit`, and `cs_content::save::library::ProfileLibrary` for
//! ids — so removing the real-file mapping, the phase order, the registry or
//! the id allocation makes them fail.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::save::codec::{MAX_SAVE_BYTES, decode, encode};
use cs_content::save::fs::{
    DirStorage, Registry, commit_registry, decode_registry, directory_sync_supported,
    load_registry, platform_note, recovery_line, registry_slot, replacement_semantics,
};
use cs_content::save::library::{ProfileLibrary, retired_name, slot_name};
use cs_content::save::store::{
    CommitError, PROFILE_PREFIX, RecoverError, SaveFile, SavePhase, SaveStorage, StorageError,
    commit, recover,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::profile::{
    ExtraField, FingerprintEntry, ProfileDocument, ProfileId, ProfileKind, RecordEntry, Revision,
    SchemaVersion, SettingApply, SettingEntry,
};

/// A disposable population directory, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-b-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    /// One population's root below the base.
    fn population(&self, kind: ProfileKind) -> PathBuf {
        self.0.join(kind.label())
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A synthetic profile document carrying a few fields of every kind, so a
/// recovered revision is compared whole rather than field by field.
fn doc(id: ProfileId, revision: u64) -> ProfileDocument {
    let mut d = ProfileDocument::synthetic(id, Revision(revision));
    d.campaign.run_id = Some("run-a".into());
    d.campaign.money_minor = 1_000 + revision;
    d.campaign
        .applied_outcomes
        .push(format!("outcome-{revision}"));
    d.blueprints
        .push(ContentId::from_source(ContentKind::Airframe, "synthetic-one").expect("id"));
    d.records.push(RecordEntry {
        key: "kills".into(),
        value: revision,
    });
    d.settings.push(SettingEntry {
        key: "display.mode".into(),
        apply: SettingApply::RestartRequired,
        value: "window=ed".into(),
    });
    d.fingerprints.push(FingerprintEntry {
        name: "catalog".into(),
        hash: 0xdead_beef_0123_4567,
    });
    d
}

/// The bytes a slot must hold for a revision, and a check that it does.
fn assert_slot_holds(directory: &Path, expected: &ProfileDocument, context: &str) {
    let slot = DirStorage::new(directory, PROFILE_PREFIX);
    let found = recover(&slot).unwrap_or_else(|e| panic!("{context}: {e}"));
    let found = found.unwrap_or_else(|| panic!("{context}: no revision found"));
    assert_eq!(&found.document, expected, "{context}: whole revision");
    assert!(
        found.warnings.is_empty(),
        "{context}: unexpected warnings {:?}",
        found.warnings
    );
}

// --- AC02: corrupt the newest file, recover a valid backup, say so ----------

/// The stage's minimum scenario on the real filesystem: damage the current
/// file, and the library recovers the backup whole, reports it as text a caller
/// can display, and keeps the damaged file for the owner to inspect.
#[test]
fn accept_f48_b_corrupt_current_recovers_backup_with_a_visible_warning() {
    let base = TempBase::new("corrupt-current");
    let kind = ProfileKind::Synthetic;
    let mut library = ProfileLibrary::open(base.population(kind), kind).expect("open");

    let (id, mut document) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    for revision in 2..=3 {
        document.revision = Revision(revision);
        document.campaign.money_minor = 1_000 + revision;
        library.save(&document).expect("save");
    }
    let directory = library.slot_dir(id);

    // Damage the newest file the way a torn write or a bad sector would: one
    // flipped byte, same length.
    let current = directory.join("profile.sav");
    let mut damaged = fs::read(&current).expect("current is readable");
    damaged[40] ^= 0x55;
    fs::write(&current, &damaged).expect("the damaged bytes are written");

    // Recovery: the newest whole revision is the backup, and the fallback is
    // text rather than only a typed value.
    let loaded = library.load(id).expect("load");
    assert_eq!(loaded.source, Some(SaveFile::Backup), "from the backup");
    assert_eq!(
        loaded.document.as_ref().expect("a revision").revision,
        Revision(2),
        "the whole previous revision, not a half of each"
    );
    let warning_text = loaded.warnings.join(" | ");
    assert!(
        warning_text.contains("profile.sav") && warning_text.contains("profile.bak"),
        "both the damaged file and the source are named: {warning_text}"
    );
    assert!(!warning_text.is_empty(), "a fallback is never silent");

    // The damaged file is not deleted: a player can recover it, and a later
    // write must not have destroyed the evidence.
    assert_eq!(fs::read(&current).expect("still there"), damaged);
    assert!(
        !directory.join("profile.tmp").exists(),
        "recovery leaves no temp file behind"
    );

    // Writing on top of the damaged file keeps the good backup until the new
    // file is installed: the damaged current is not rotated, so the previous
    // valid revision is still the backup after the write.
    document.revision = Revision(4);
    document.campaign.money_minor = 1_004;
    library
        .save(&document)
        .expect("save over a damaged current");
    assert_slot_holds(&directory, &document, "after the recovery write");
    let rotated = fs::read(directory.join("profile.bak")).expect("the backup file");
    assert_eq!(
        decode(&rotated)
            .expect("the backup is a whole save")
            .revision,
        Revision(2),
        "the good backup survived the write over the damaged current"
    );
}

/// A truncated current file — the signature of a write interrupted by a full
/// disk — is the same case, and a file that is not a save at all is refused
/// rather than installed.
#[test]
fn accept_f48_b_truncated_and_unrecognised_files_are_not_installed() {
    let base = TempBase::new("truncated");
    let kind = ProfileKind::Synthetic;
    let mut library = ProfileLibrary::open(base.population(kind), kind).expect("open");
    let (id, mut document) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    document.revision = Revision(2);
    library.save(&document).expect("second");
    let directory = library.slot_dir(id);

    let valid = encode(&document).expect("encode");
    for damaged in [
        valid[..valid.len() / 2].to_vec(),
        b"not a save at all".to_vec(),
        Vec::new(),
    ] {
        fs::write(directory.join("profile.sav"), &damaged).expect("write");
        let loaded = library.load(id).expect("load");
        assert_eq!(
            loaded.document.as_ref().expect("the backup").revision,
            Revision(1),
            "a whole earlier revision is recovered"
        );
        assert!(
            !loaded.warnings.is_empty(),
            "an unreadable current file is reported"
        );
    }
}

// --- AC01: interrupt every write phase on the real filesystem ---------------

/// Every phase of a real commit, interrupted at its own boundary, leaves a
/// whole revision and a readable slot afterwards. The interruption is
/// injected through the production `SaveStorage` seam, so the phases that do
/// run are the real file operations.
#[test]
fn accept_f48_b_interrupting_any_phase_on_disk_keeps_a_whole_revision() {
    use cs_content::save::store::SaveStorage;

    /// A real `DirStorage` that stops at one phase, modelling a process killed
    /// there. The file operations before the stop are the production ones.
    struct StopAt {
        inner: DirStorage,
        phase: SavePhase,
    }

    impl SaveStorage for StopAt {
        fn read(&self, file: SaveFile) -> Result<Option<Vec<u8>>, StorageError> {
            self.inner.read(file)
        }
        fn write_temp(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
            if self.phase == SavePhase::WriteTemp {
                // A kill mid-write: half the bytes, nothing synced.
                self.inner.write_temp(&bytes[..bytes.len() / 2])?;
                return Err(StorageError::Interrupted(SavePhase::WriteTemp));
            }
            self.inner.write_temp(bytes)
        }
        fn sync_temp(&mut self) -> Result<(), StorageError> {
            if self.phase == SavePhase::SyncTemp {
                return Err(StorageError::Interrupted(SavePhase::SyncTemp));
            }
            self.inner.sync_temp()
        }
        fn rotate_backup(&mut self) -> Result<(), StorageError> {
            if self.phase == SavePhase::RotateBackup {
                return Err(StorageError::Interrupted(SavePhase::RotateBackup));
            }
            self.inner.rotate_backup()
        }
        fn install_current(&mut self) -> Result<(), StorageError> {
            if self.phase == SavePhase::InstallCurrent {
                return Err(StorageError::Interrupted(SavePhase::InstallCurrent));
            }
            self.inner.install_current()
        }
        fn sync_dir(&mut self) -> Result<(), StorageError> {
            if self.phase == SavePhase::SyncDir {
                return Err(StorageError::Interrupted(SavePhase::SyncDir));
            }
            self.inner.sync_dir()
        }
    }

    for phase in SavePhase::ALL {
        let base = TempBase::new("interrupt");
        let id = ProfileId::new(1).expect("nonzero");
        let directory = base.0.join(slot_name(id));
        let mut slot = DirStorage::create(&directory, PROFILE_PREFIX).expect("slot");
        commit(&mut slot, &doc(id, 1)).expect("first");
        commit(&mut slot, &doc(id, 2)).expect("second");

        let mut stopped = StopAt {
            inner: DirStorage::new(&directory, PROFILE_PREFIX),
            phase,
        };
        let err = commit(&mut stopped, &doc(id, 3))
            .expect_err("the interrupted commit reports its phase");
        assert_eq!(err, CommitError::Storage(StorageError::Interrupted(phase)));

        // Reopening after the kill always yields a whole revision, and it is
        // one of the two the process was between — never a mixture.
        let found = recover(&DirStorage::new(&directory, PROFILE_PREFIX))
            .unwrap_or_else(|e| panic!("{phase:?}: reopen: {e}"))
            .unwrap_or_else(|| panic!("{phase:?}: a whole revision survives"));
        let revision = found.document.revision;
        assert!(
            revision == Revision(2) || revision == Revision(3),
            "{phase:?}: got {revision:?}"
        );
        assert_eq!(found.document, doc(id, revision.0), "{phase:?}: whole");

        // And the next commit succeeds and wins, with a valid backup left.
        let mut slot = DirStorage::new(&directory, PROFILE_PREFIX);
        commit(&mut slot, &doc(id, 4)).expect("recommit");
        assert_slot_holds(&directory, &doc(id, 4), &format!("{phase:?} recommit"));
        let backup = recover(&DirStorage::new(&directory, PROFILE_PREFIX))
            .expect("recover")
            .expect("a revision");
        assert!(
            backup.document.revision >= Revision(2),
            "{phase:?}: the backup is not older than the state before the crash"
        );
    }
}

/// A commit that is killed before its temp file is synced leaves a temp file
/// that recovery refuses, rather than a revision the player sees and a later
/// write treats as newer.
#[test]
fn accept_f48_b_an_unsynced_temp_file_is_never_treated_as_a_revision() {
    let base = TempBase::new("unsynced-temp");
    let id = ProfileId::new(4).expect("nonzero");
    let directory = base.0.join(slot_name(id));
    let mut slot = DirStorage::create(&directory, PROFILE_PREFIX).expect("slot");
    commit(&mut slot, &doc(id, 1)).expect("first");
    // The bytes of a revision that was written but never installed, then
    // truncated by the same interruption a killed process leaves.
    let mut complete = encode(&doc(id, 2)).expect("encode");
    complete.truncate(complete.len() / 2);
    fs::write(directory.join("profile.tmp"), &complete).expect("a half-written temp");

    let found = recover(&DirStorage::new(&directory, PROFILE_PREFIX))
        .expect("recover")
        .expect("the installed revision");
    assert_eq!(found.document.revision, Revision(1));
    assert!(
        found.warnings.iter().any(|w| matches!(
            w,
            cs_content::save::store::RecoveryWarning::Corrupt {
                file: SaveFile::Temp,
                ..
            }
        )),
        "the half-written temp is reported: {:?}",
        found.warnings
    );

    // Writing again replaces the debris and does not lose the installed one.
    let mut slot = DirStorage::new(&directory, PROFILE_PREFIX);
    commit(&mut slot, &doc(id, 3)).expect("commit over debris");
    assert_slot_holds(&directory, &doc(id, 3), "after debris");
}

// --- AC03: create/delete/create never reissues an id -----------------------

/// Ids come from the persisted high-water mark, so deleting the highest id and
/// creating again moves past it — across a restart, which is the case a
/// list-index identity fails.
#[test]
fn accept_f48_b_deleted_ids_are_never_reissued_across_a_restart() {
    let base = TempBase::new("ids");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");

    let (first, second, third, highest);
    {
        let mut library = ProfileLibrary::open(&population, kind).expect("open");
        let (a, _) = library.create(doc(seed, 1)).expect("create a");
        let (b, _) = library.create(doc(seed, 1)).expect("create b");
        let (c, _) = library.create(doc(seed, 1)).expect("create c");
        // Delete the highest id, and one that is not.
        library.delete(c).expect("delete the highest");
        library.delete(a).expect("delete the lowest");
        let (d, _) = library.create(doc(seed, 1)).expect("create after delete");
        assert!(
            d > c,
            "the new id is above every id ever issued: {d} vs {c}"
        );
        assert_eq!(library.live(), &[b, d], "the live list holds what is left");
        first = a;
        second = b;
        third = c;
        highest = d;
    }

    // A restart reads the persisted mark, not the surviving slots.
    let library = ProfileLibrary::open(&population, kind).expect("reopen");
    assert_eq!(library.live(), &[second, highest], "the live list survived");
    assert!(library.status().high_water >= highest.get());
    let mut library = library;
    let (next, _) = library.create(doc(seed, 1)).expect("create after restart");
    assert!(
        next > highest,
        "a restarted build still allocates above the mark: {next}"
    );
    for gone in [first, third] {
        assert!(!library.live().contains(&gone), "{gone} is not live again");
        let loaded = library.load(gone).expect("load");
        assert!(
            !loaded.is_present(),
            "{gone} names no live profile: its slot is retired"
        );
        // The retired slot is kept, not destroyed: the id stays named on disk,
        // which is what makes a regressed registry unable to reissue it.
        assert!(
            library.base().join(retired_name(gone)).is_dir(),
            "{gone}'s files are kept under a retired name"
        );
    }
    assert!(
        library.load(next).expect("load").is_present(),
        "the new id reads back its own profile"
    );
}

/// The active pointer is persisted with the registry and survives a restart,
/// and a pointer to a profile that is not live is dropped rather than trusted.
#[test]
fn accept_f48_b_the_active_pointer_is_persisted_and_never_dangling() {
    let base = TempBase::new("active");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");

    let (a, b);
    {
        let mut library = ProfileLibrary::open(&population, kind).expect("open");
        let (first, _) = library.create(doc(seed, 1)).expect("create a");
        let (second, _) = library.create(doc(seed, 1)).expect("create b");
        assert_eq!(library.active(), Some(first), "the first is active");
        library.set_active(second).expect("set active");
        a = first;
        b = second;
    }
    let library = ProfileLibrary::open(&population, kind).expect("reopen");
    assert_eq!(library.active(), Some(b), "the pointer survived");

    let mut library = library;
    library.delete(b).expect("delete the active profile");
    assert_ne!(
        library.active(),
        Some(b),
        "a deleted profile is not left as the active pointer"
    );
    assert!(library.active().is_none_or(|id| id == a));

    // A registry that names an active profile which is not live is repaired on
    // read rather than trusted, and the repair is the same as a fresh pointer
    // to the first live profile.
    drop(library);
    let slot = registry_slot(&population);
    fs::write(
        slot.path(SaveFile::Current),
        seal(&format!(
            "CSREG 1.0\nkind=synthetic\nrevision=99\nhigh_water={}\nlive={a}\nactive={b}\n",
            b.get()
        )),
    )
    .expect("write");
    fs::remove_file(slot.path(SaveFile::Backup)).ok();
    let repaired = ProfileLibrary::open(&population, kind).expect("open with a dangling pointer");
    assert_eq!(
        repaired.active(),
        None,
        "an active pointer to a deleted profile is not adopted"
    );
    assert_eq!(
        repaired.live(),
        &[a],
        "the live set is what the record says"
    );
}

/// A registry that cannot be read at all is recovered from the slots' own ids
/// rather than treated as an empty population, and a population that was never
/// written stays an error-free empty library.
#[test]
fn accept_f48_b_an_unreadable_registry_is_recovered_from_the_slots() {
    let base = TempBase::new("unreadable-registry");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");

    let mut library = ProfileLibrary::open(&population, kind).expect("open");
    let (first, _) = library.create(doc(seed, 1)).expect("create a");
    let (second, _) = library.create(doc(seed, 1)).expect("create b");
    library.delete(second).expect("delete the highest");

    // Destroy every readable copy of the registry: the population still exists,
    // so it must not come back empty and hand out id 2 again.
    let slot = registry_slot(&population);
    for file in SaveFile::ALL {
        fs::remove_file(slot.path(file)).ok();
    }
    fs::write(slot.path(SaveFile::Current), b"garbage").expect("write garbage");

    let mut recovered = ProfileLibrary::open(&population, kind).expect("recovered from the slots");
    assert_eq!(
        recovered.status().high_water,
        second.get(),
        "the mark comes from the ids the directory names, live or retired"
    );
    assert_eq!(
        recovered.live(),
        &[first],
        "only the live slot is a profile"
    );
    assert!(
        !recovered.status().registry_warnings.is_empty(),
        "the unreadable registry is reported: {:?}",
        recovered.status().registry_warnings
    );
    let mut next = doc(seed, 1);
    next.kind = kind;
    let (issued, _) = recovered.create(next).expect("create after recovery");
    assert!(
        issued > second,
        "an id issued before the loss is not issued again: {issued} vs {second}"
    );
}

// --- Population separation -------------------------------------------------

/// A profile of one population is invisible to another, and a registry
/// belonging to another population is refused rather than adopted, so a
/// synthetic or evidence session can never reach a production profile.
#[test]
fn accept_f48_b_populations_are_separate_and_a_foreign_registry_is_refused() {
    let base = TempBase::new("populations");
    let seed = ProfileId::new(1).expect("nonzero");

    let mut production = ProfileLibrary::open(
        base.population(ProfileKind::Production),
        ProfileKind::Production,
    )
    .expect("open production");
    let mut live_document = doc(seed, 1);
    live_document.kind = ProfileKind::Production;
    let (live, _) = production
        .create(live_document)
        .expect("create a production profile");

    let synthetic = ProfileLibrary::open(
        base.population(ProfileKind::Synthetic),
        ProfileKind::Synthetic,
    )
    .expect("open synthetic");
    assert!(
        synthetic.live().is_empty(),
        "another population sees nothing"
    );
    assert!(
        !synthetic.load(live).expect("load").is_present(),
        "a production slot is not readable as a synthetic one"
    );

    // A registry of the wrong population in a directory is refused, and its
    // bytes are left alone.
    let evidence = base.population(ProfileKind::Evidence);
    let mut wrong = DirStorage::create(&evidence, "registry").expect("create");
    let foreign = Registry::empty(ProfileKind::Production);
    let written = commit_registry(&mut wrong, &foreign).expect("write");
    let held = fs::read(registry_slot(&evidence).path(SaveFile::Current)).expect("read back");
    let refused = load_registry(&evidence, ProfileKind::Evidence);
    assert!(
        matches!(refused, Err(RecoverError::RegistryKindMismatch { .. })),
        "a foreign registry is refused: {refused:?}"
    );
    assert_eq!(
        fs::read(registry_slot(&evidence).path(SaveFile::Current)).expect("read back"),
        held,
        "the refused registry is not overwritten"
    );
    assert!(written.kind() == ProfileKind::Production);
}

// --- AC04: hostile input never reaches the filesystem destructively ---------

/// Future-schema, oversized and non-UTF-8 files in a slot are refused by name
/// and never replaced. A newer build's data must survive an older build
/// running on the same directory.
#[test]
fn accept_f48_b_foreign_and_hostile_files_are_refused_not_overwritten() {
    let base = TempBase::new("hostile");
    let kind = ProfileKind::Synthetic;
    let mut library = ProfileLibrary::open(base.population(kind), kind).expect("open");
    let (id, mut document) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    let directory = library.slot_dir(id);
    let slot = DirStorage::new(&directory, PROFILE_PREFIX);

    // A file from a future major version: recovery refuses the whole slot and
    // no write is attempted over it.
    let future = b"CSSAVE 9.0\nprofile_id=1\nkind=synthetic\ndisplay_name=Future\nrevision=99\n";
    fs::write(directory.join("profile.sav"), future).expect("write");
    assert!(matches!(
        recover(&slot),
        Err(RecoverError::UnsupportedMajor { .. })
    ));
    document.revision = Revision(2);
    assert!(
        library.save(&document).is_err(),
        "a future save is never overwritten by this build"
    );
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        future,
        "the newer build's bytes are untouched"
    );

    // An oversized file is refused by the read, before it is parsed.
    let huge = vec![b'a'; MAX_SAVE_BYTES + 1];
    fs::write(directory.join("profile.sav"), &huge).expect("write");
    let err = recover(&slot).expect_err("oversized");
    assert!(
        err.to_string().contains("over"),
        "an oversized file names the bound: {err}"
    );
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        huge
    );

    // Non-UTF-8 bytes are refused the same way.
    let binary = vec![0xff_u8; 64];
    fs::write(directory.join("profile.sav"), &binary).expect("write");
    assert!(recover(&slot).is_err());
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        binary
    );
}

// --- Refusals a stale or foreign write must not overwrite ------------------

/// A revision that is not above the stored one, a document from another
/// population and an id that is not live are all refused, and a refused write
/// leaves the slot byte-identical.
#[test]
fn accept_f48_b_stale_and_foreign_writes_are_refused_without_a_write() {
    let base = TempBase::new("refusals");
    let kind = ProfileKind::Synthetic;
    let mut library = ProfileLibrary::open(base.population(kind), kind).expect("open");
    let (id, mut document) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    let directory = library.slot_dir(id);
    let held = fs::read(directory.join("profile.sav")).expect("read");

    // The same revision again.
    assert!(matches!(
        library.save(&document),
        Err(LibraryErrorRefusal::Commit(
            CommitError::RevisionConflict { .. }
        ))
    ));
    // An older one.
    let mut older = document.clone();
    older.revision = Revision(0);
    assert!(library.save(&older).is_err());
    // A document of another population.
    let mut foreign = document.clone();
    foreign.kind = ProfileKind::Evidence;
    assert!(library.save(&foreign).is_err(), "a foreign document");
    // An id that is not live in this population.
    let mut unknown = document.clone();
    unknown.profile_id = ProfileId::new(99).expect("nonzero");
    assert!(library.save(&unknown).is_err(), "an unlisted id");
    // A document that cannot be encoded at all.
    let mut invalid = document.clone();
    invalid.display_name = "line\nbreak".into();
    assert!(library.save(&invalid).is_err(), "an unencodable document");

    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("read"),
        held,
        "every refusal left the slot byte-identical"
    );

    // And the next valid revision still succeeds.
    document.revision = Revision(2);
    library.save(&document).expect("the valid write");
    assert_slot_holds(&directory, &document, "after the refusals");
}

/// Alias so the refusal assertions above read as the library's own error type
/// without importing it under a shadowed name.
type LibraryErrorRefusal = cs_content::save::library::LibraryError;

// --- The registry itself ---------------------------------------------------

/// The registry is a checksummed document with the same bounds as a save, and
/// it round-trips exactly — the high-water mark included.
#[test]
fn accept_f48_b_the_registry_round_trips_and_is_bounded() {
    let base = TempBase::new("registry");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");

    let (first, second);
    {
        let mut library = ProfileLibrary::open(&population, kind).expect("open");
        let (a, _) = library.create(doc(seed, 1)).expect("create a");
        let (b, _) = library.create(doc(seed, 1)).expect("create b");
        first = a;
        second = b;
    }
    let loaded = load_registry(&population, kind).expect("load");
    assert!(loaded.was_persisted, "the registry is a real file");
    assert!(loaded.warnings.is_empty());
    assert_eq!(loaded.registry.live(), &[first, second]);
    assert_eq!(loaded.registry.high_water(), second.get());
    assert_eq!(loaded.registry.active(), Some(first));
    assert_eq!(
        loaded.registry.revision(),
        Revision(2),
        "two writes, two revisions"
    );

    // The bytes on disk decode to exactly the registry in force.
    let slot = registry_slot(&population);
    let held = fs::read(slot.path(SaveFile::Current)).expect("read");
    let (revision, decoded) = decode_registry(&held).expect("decode");
    assert_eq!(revision, Revision(2));
    assert_eq!(decoded, loaded.registry);

    // Damaged bytes are refused by name, and the backup takes over: the
    // registry in force is the previous whole revision, not a mixture.
    let mut damaged = held.clone();
    let index = damaged.len() / 2;
    damaged[index] ^= 0xff;
    fs::write(slot.path(SaveFile::Current), &damaged).expect("write");
    let after = load_registry(&population, kind).expect("load after damage");
    assert_eq!(
        after.registry.revision(),
        Revision(1),
        "the previous whole registry revision, not a half of each"
    );
    assert!(
        after
            .warnings
            .iter()
            .any(|w| matches!(w, cs_content::save::store::RecoveryWarning::Corrupt { .. })),
        "the damaged registry is reported: {:?}",
        after.warnings
    );
    assert!(
        after.warnings.iter().any(|w| matches!(
            w,
            cs_content::save::store::RecoveryWarning::UsedFallback { .. }
        )),
        "the fallback is visible: {:?}",
        after.warnings
    );

    // The library does not accept that regressed mark: the second profile's
    // slot is on disk, so the mark is raised and the profile adopted rather
    // than its id being handed out again.
    let mut library = ProfileLibrary::open(&population, kind).expect("open after damage");
    assert!(
        library.status().high_water >= second.get(),
        "the regressed mark is raised to what the directory shows: {}",
        library.status().high_water
    );
    assert!(
        library.live().contains(&second),
        "the profile whose registry write was lost is adopted: {:?}",
        library.live()
    );
    assert!(
        !library.status().notices.is_empty(),
        "the reconciliation is reported, not silent"
    );
    let mut next = doc(ProfileId::new(1).expect("nonzero"), 1);
    next.kind = kind;
    let (issued, _) = library.create(next).expect("create after damage");
    assert!(
        issued > second,
        "an adopted id is not reissued: {issued} vs {second}"
    );

    // A registry that decodes but does not hold together is refused as a whole
    // file, not adopted with a lower mark — that is what would reissue a
    // deleted id.
    for body in [
        // A mark below a live id.
        "CSREG 1.0\nkind=synthetic\nrevision=9\nhigh_water=1\nlive=7\nactive=7\n",
        "CSREG 1.0\nkind=synthetic\nrevision=9\nhigh_water=2\nlive=5\n",
        // A live id listed twice.
        "CSREG 1.0\nkind=synthetic\nrevision=9\nhigh_water=7\nlive=3\nlive=3\n",
        // A required field missing, and a field this build does not know.
        "CSREG 1.0\nkind=synthetic\nrevision=9\nlive=3\n",
        "CSREG 1.0\nkind=synthetic\nrevision=9\nhigh_water=3\nfuture.field=1\n",
        // A field that is not the value it claims to be.
        "CSREG 1.0\nkind=nowhere\nrevision=9\nhigh_water=3\n",
    ] {
        let sealed = seal(body);
        fs::write(slot.path(SaveFile::Current), &sealed).expect("write");
        fs::remove_file(slot.path(SaveFile::Backup)).ok();
        let held = load_registry(&population, kind);
        assert!(
            matches!(held, Err(RecoverError::NoValidSave { .. })),
            "an inconsistent registry is refused: {body:?} -> {held:?}"
        );
    }
}

/// A registry with a mark below a live id is refused, so a hand-edited or
/// truncated registry cannot cause an id to be issued twice.
#[test]
fn accept_f48_b_a_registry_with_a_lowered_mark_is_refused() {
    let base = TempBase::new("lowered-mark");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");
    {
        let mut library = ProfileLibrary::open(&population, kind).expect("open");
        for _ in 0..3 {
            library.create(doc(seed, 1)).expect("create");
        }
    }
    let slot = registry_slot(&population);
    fs::write(
        slot.path(SaveFile::Current),
        seal("CSREG 1.0\nkind=synthetic\nrevision=9\nhigh_water=1\nlive=3\n"),
    )
    .expect("write");
    fs::remove_file(slot.path(SaveFile::Backup)).ok();
    let held = load_registry(&population, kind);
    assert!(
        matches!(held, Err(RecoverError::NoValidSave { .. })),
        "a mark below a live id is refused: {held:?}"
    );
}

/// The byte bound applies to the registry too, so a huge file in the registry
/// slot is refused by the read rather than parsed.
#[test]
fn accept_f48_b_an_oversized_registry_is_refused() {
    let base = TempBase::new("big-registry");
    let population = base.population(ProfileKind::Synthetic);
    let slot = DirStorage::create(&population, "registry").expect("create");
    let huge = vec![b'a'; MAX_SAVE_BYTES + 1];
    fs::write(slot.path(SaveFile::Current), &huge).expect("write");
    let held = load_registry(&population, ProfileKind::Synthetic);
    assert!(held.is_err(), "an oversized registry is refused");
}

// --- Platform reporting ----------------------------------------------------

/// The write path reports what this platform does rather than assuming POSIX
/// semantics, and the statement is about the platform, not about a claim that
/// was tested everywhere.
#[test]
fn accept_f48_b_the_platform_write_path_is_reported_not_assumed() {
    let note = platform_note();
    assert!(note.contains(&replacement_semantics().to_string()));
    assert!(note.contains(if directory_sync_supported() {
        "supported"
    } else {
        "no-op"
    }));
    // A directory sync, where the platform has one, is a real call: a commit
    // that cannot sync the directory is a failure, not a silent success.
    let base = TempBase::new("platform");
    let id = ProfileId::new(1).expect("nonzero");
    let mut slot = DirStorage::create(base.0.join(slot_name(id)), PROFILE_PREFIX).expect("slot");
    commit(&mut slot, &doc(id, 1)).expect("commit with a directory sync");
    assert!(
        fs::metadata(slot.directory()).expect("metadata").is_dir(),
        "the slot is a real directory"
    );
}

/// A slot path that is not a plain directory is refused, so a save never writes
/// through a symbolic link or into a regular file.
#[test]
fn accept_f48_b_a_slot_that_is_not_a_directory_is_refused() {
    let base = TempBase::new("not-a-dir");
    let id = ProfileId::new(1).expect("nonzero");
    let file = base.0.join(slot_name(id));
    fs::write(&file, b"a file where a slot should be").expect("write");
    assert!(
        DirStorage::create(&file, PROFILE_PREFIX).is_err(),
        "a regular file is not a slot"
    );
    assert!(
        DirStorage::new(&file, PROFILE_PREFIX)
            .read(SaveFile::Current)
            .is_err()
    );
    assert_eq!(
        fs::read(&file).expect("read back"),
        b"a file where a slot should be",
        "the file is untouched"
    );
}

// --- Recovery text ---------------------------------------------------------

/// A library's recovery diagnostics are text a caller can display, not only a
/// typed value it might drop.
#[test]
fn accept_f48_b_recovery_is_reported_as_text() {
    let base = TempBase::new("text");
    let kind = ProfileKind::Synthetic;
    let population = base.population(kind);
    let seed = ProfileId::new(1).expect("nonzero");
    let mut library = ProfileLibrary::open(&population, kind).expect("open");
    let (id, mut document) = library.create(doc(seed, 1)).expect("create");
    document.revision = Revision(2);
    library.save(&document).expect("second");

    let directory = library.slot_dir(id);
    let current = directory.join("profile.sav");
    let mut damaged = fs::read(&current).expect("read");
    damaged[10] ^= 0x20;
    fs::write(&current, &damaged).expect("write");

    let loaded = library.load(id).expect("load");
    let text = recovery_line(&library.slot_warnings(id).expect("warnings"));
    assert_ne!(text, "no recovery was needed");
    assert_eq!(text, loaded.warnings.join("; "));
    assert!(text.contains("profile.sav"), "{text}");
}

/// A population that was never written has no registry file, and that is
/// reported as an empty library with a mark of zero rather than as a failure.
#[test]
fn accept_f48_b_an_unwritten_population_is_empty_not_broken() {
    let base = TempBase::new("unwritten");
    let kind = ProfileKind::Synthetic;
    let library = ProfileLibrary::open(base.population(kind), kind).expect("open");
    assert!(library.live().is_empty());
    assert_eq!(library.active(), None);
    assert_eq!(library.status().high_water, 0);
    assert!(!library.status().registry_persisted);
    assert!(library.status().registry_warnings.is_empty());
}

// --- Helpers ---------------------------------------------------------------

/// Re-seals hand-edited text with a correct checksum line.
fn seal(body: &str) -> Vec<u8> {
    use cs_content::save::codec::checksum;
    format!("{body}checksum={:016x}\n", checksum(body.as_bytes())).into_bytes()
}

/// A save that round-trips through the production codec, so the fixtures above
/// are not built by a second encoder.
#[test]
fn accept_f48_b_the_fixture_documents_survive_the_production_codec() {
    let id = ProfileId::new(1).expect("nonzero");
    let document = doc(id, 3);
    let bytes = encode(&document).expect("encode");
    assert_eq!(decode(&bytes).expect("decode"), document);
    let mut with_unknown = document.clone();
    with_unknown.schema = SchemaVersion { major: 1, minor: 4 };
    with_unknown.extra.push(ExtraField {
        key: "future.widget".into(),
        value: "kept".into(),
    });
    let again = decode(&encode(&with_unknown).expect("encode")).expect("decode");
    assert_eq!(
        again.extra, with_unknown.extra,
        "unknown fields are preserved"
    );
}
