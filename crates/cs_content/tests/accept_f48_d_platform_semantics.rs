//! Acceptance scenarios F48-D, the platform-semantics rows: the cases where
//! `rename(2)` and `MoveFileExW` are documented to differ, run on whichever
//! platform the suite is on. Task test prefix: `accept_f48_d_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`, stage
//! `### F48-D`; contract `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence":
//! "Windows replacement semantics require an actual platform test rather than
//! assuming POSIX rename behavior").
//!
//! The crash/recovery matrix in `accept_f48_d_crash_recovery_matrix.rs` is
//! platform-agnostic: it kills a real process and checks what survives, and its
//! assertions hold wherever the write path is correct. The cases here are the
//! opposite — they exist *because* the platforms are documented to differ:
//!
//! - a `rename` over a read-only destination: POSIX cares about the containing
//!   directory's permissions, so it succeeds; `MoveFileExW` with
//!   `MOVEFILE_REPLACE_EXISTING` refuses a read-only destination;
//! - a `rename` over a destination held open: POSIX succeeds and the open
//!   handle keeps the replaced inode. `std::fs::File::open` requests delete
//!   sharing on Windows, so `MoveFileExW` is *expected* to replace it there
//!   too — the run either confirms that or produces the finding. A handle
//!   opened *without* delete sharing (another program holding the file) is a
//!   different case `std` cannot express portably, and remains unmeasured;
//! - a directory-handle sync: real on unix, a documented no-op off unix, where
//!   the raw `File::open(directory)` it would need is itself unavailable.
//!
//! The unix assertions below are measured on macOS arm64 and Linux x86-64
//! (see `docs/findings/2026-10-01-f48-d-crash-recovery-matrix-xplat.md`). The
//! `cfg!(windows)` arms are written from documented `MoveFileExW` and `std`
//! sharing semantics and have not run: a Windows run either confirms them or
//! produces the finding that the documentation was wrong — either is the
//! result this stage wants.
//!
//! Every byte written here is newly authored synthetic data under the system
//! temporary directory, removed when the test finishes. No test touches a real
//! user profile directory, `$CS_GAME_DIR` or any original data.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::save::codec::decode;
use cs_content::save::fs::{
    DirStorage, directory_sync_supported, platform_note, replacement_semantics,
};
use cs_content::save::store::{
    PROFILE_PREFIX, RecoverError, RecoveryWarning, SaveFile, SaveStorage, commit, recover,
};
use cs_types::profile::{ProfileDocument, ProfileId, Revision};

/// A disposable population directory, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-d-xplat-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn doc(id: ProfileId, revision: u64) -> ProfileDocument {
    ProfileDocument::synthetic(id, Revision(revision))
}

/// Marks a file read-only the way this platform spells it, so the test is the
/// same setup on every OS rather than a unix-only probe.
#[cfg(unix)]
fn set_read_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .expect("the file's metadata")
        .permissions();
    permissions.set_mode(0o444);
    fs::set_permissions(path, permissions).expect("the file is marked read-only");
}

/// `Permissions::set_readonly` is the portable form; it maps to the Windows
/// read-only file attribute, which is what `MoveFileExW` honours.
#[cfg(not(unix))]
fn set_read_only(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the file's metadata")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).expect("the file is marked read-only");
}

/// Restores writability so cleanup can remove the file on platforms where a
/// read-only file cannot be deleted.
#[cfg(unix)]
fn set_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .expect("the file's metadata")
        .permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(path, permissions).expect("the file is writable again");
}

#[cfg(not(unix))]
fn set_writable(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the file's metadata")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("the file is writable again");
}

/// What a platform's `rename` does when a commit's replacement step meets the
/// prepared destination: either the commit completes and `profile.sav` holds
/// the offered revision, or the commit is refused and the uninstalled temp
/// file still carries it whole. The second outcome is a refusal, not
/// corruption — the caller gets an error and the reader still gets a whole
/// revision.
#[derive(Debug)]
enum ReplaceOutcome {
    /// The commit returned `Ok`: the offered revision is installed.
    Replaced,
    /// The commit returned `Err`: the platform refused the replacement, and
    /// the offered revision survives as the uninstalled temp file.
    Refused,
}

/// Runs a commit whose replacement step meets a prepared destination, then
/// reports what happened. `prepare` runs before the commit and `restore`
/// afterwards, so the fixture is left deletable whatever the platform decided.
fn replace_over(
    slot_dir: &Path,
    id: ProfileId,
    offered: u64,
    prepare: impl Fn(),
    restore: impl Fn(),
) -> ReplaceOutcome {
    prepare();
    let outcome = commit(
        &mut DirStorage::new(slot_dir, PROFILE_PREFIX),
        &doc(id, offered),
    );
    restore();

    let (recovered, source, warnings) = match recover(&DirStorage::new(slot_dir, PROFILE_PREFIX)) {
        Ok(found) => found.map_or((None, None, Vec::new()), |found| {
            (
                Some(found.document.revision.0),
                Some(found.source),
                found.warnings,
            )
        }),
        Err(RecoverError::NoValidSave { .. }) => (None, None, Vec::new()),
        Err(other) => panic!("reopening after the commit: {other}"),
    };

    match outcome {
        Ok(()) => {
            assert_eq!(
                (recovered, source),
                (Some(offered), Some(SaveFile::Current)),
                "a commit that returned Ok must have installed the offered revision"
            );
            ReplaceOutcome::Replaced
        }
        Err(error) => {
            println!("the platform refused the replacement: {error}");
            // A refused commit leaves the offered revision as a whole but
            // uninstalled temp file: recovery selects it and says so.
            assert_eq!(
                recovered,
                Some(offered),
                "a refused commit must still leave a whole revision to read"
            );
            assert_eq!(source, Some(SaveFile::Temp));
            assert!(
                warnings
                    .iter()
                    .any(|warning| matches!(warning, RecoveryWarning::UsedFallback { .. })),
                "a recovered temp file is reported, not silent: {warnings:?}"
            );
            ReplaceOutcome::Refused
        }
    }
}

/// A `rename` over a read-only destination file. On unix the rename's
/// permissions are the containing directory's, so the commit completes. The
/// documented `MoveFileExW` behaviour is a refusal: `MOVEFILE_REPLACE_EXISTING`
/// does not replace a read-only destination, so the commit errors and the
/// offered revision is recoverable from the uninstalled temp file.
///
/// The read-only file is the *destination* of `rotate_backup`'s replace
/// (`profile.bak`), not the current file: a read-only `profile.sav` alone does
/// not discriminate, because it is only ever the *source* of the rotate rename
/// and is vacated before `install_current` replaces its name.
#[test]
fn accept_f48_d_replacement_over_a_read_only_destination_is_the_platforms_semantics() {
    let base = TempBase::new("read-only-dest");
    let id = ProfileId::new(1).expect("nonzero");
    let slot_dir = base.0.join("profile-1");
    let mut slot = DirStorage::create(&slot_dir, PROFILE_PREFIX).expect("the slot");
    commit(&mut slot, &doc(id, 1)).expect("first");
    commit(&mut DirStorage::new(&slot_dir, PROFILE_PREFIX), &doc(id, 2)).expect("second");

    let bak = slot_dir.join(SaveFile::Backup.prefixed_name(PROFILE_PREFIX));
    let outcome = replace_over(
        &slot_dir,
        id,
        3,
        || set_read_only(&bak),
        || set_writable(&bak),
    );

    if cfg!(unix) {
        assert!(
            matches!(outcome, ReplaceOutcome::Replaced),
            "rename(2) replaces a read-only destination (measured on macOS and Linux)"
        );
    } else {
        assert!(
            matches!(outcome, ReplaceOutcome::Refused),
            "MoveFileExW does not replace a read-only destination (documented, \
             unmeasured: no Windows host has run this)"
        );
    }
    println!(
        "rename over a read-only destination on {}: {outcome:?} ({})",
        std::env::consts::OS,
        replacement_semantics()
    );

    // Whatever the platform decided, the slot is not wedged: with the
    // read-only mark gone the next commit succeeds.
    commit(&mut DirStorage::new(&slot_dir, PROFILE_PREFIX), &doc(id, 4))
        .expect("the slot accepts commits again");
}

/// A `rename` over a destination held open. On unix the replacement succeeds
/// and the open handle keeps the replaced inode — the file's name and its old
/// bytes part ways. `std::fs::File::open` requests delete sharing on Windows,
/// so `MoveFileExW` is expected to replace it there too, and the open handle
/// keeps the replaced file's bytes on both.
///
/// What this cannot express is the hostile case: a handle opened *without*
/// delete sharing (another program holding the save open) cannot be produced
/// through `std` portably, so that case is recorded as unmeasured rather than
/// asserted here.
#[test]
fn accept_f48_d_replacement_over_an_open_destination_is_the_platforms_semantics() {
    let base = TempBase::new("open-dest");
    let id = ProfileId::new(1).expect("nonzero");
    let slot_dir = base.0.join("profile-1");
    let mut slot = DirStorage::create(&slot_dir, PROFILE_PREFIX).expect("the slot");
    commit(&mut slot, &doc(id, 1)).expect("first");
    commit(&mut DirStorage::new(&slot_dir, PROFILE_PREFIX), &doc(id, 2)).expect("second");

    let bak = slot_dir.join(SaveFile::Backup.prefixed_name(PROFILE_PREFIX));
    let mut held = File::open(&bak).expect("the destination is held open");
    let outcome = replace_over(&slot_dir, id, 3, || {}, || {});

    assert!(
        matches!(outcome, ReplaceOutcome::Replaced),
        "rename(2) replaces an open destination; MoveFileExW is expected to as \
         well because std::fs::File::open requests delete sharing"
    );
    // The held handle still reads the replaced bytes: revision 1, although
    // `profile.bak` now names revision 2 and `profile.sav` revision 3.
    let mut bytes = Vec::new();
    held.read_to_end(&mut bytes)
        .expect("the replaced file still reads through the open handle");
    assert_eq!(
        decode(&bytes)
            .expect("the replaced bytes still decode")
            .revision,
        Revision(1),
        "the open handle keeps the replaced file's bytes"
    );
    println!(
        "rename over an open destination on {}: {outcome:?} ({})",
        std::env::consts::OS,
        replacement_semantics()
    );
}

/// The `SyncDir` phase: the production call must return `Ok` on every platform
/// (off unix it is the documented no-op `directory_sync_supported()` reports),
/// and the raw probe records whether a directory handle can even be opened and
/// synced — the operation the phase's no-op stands in for.
#[test]
fn accept_f48_d_the_directory_sync_phase_is_a_real_call_or_a_recorded_no_op() {
    let base = TempBase::new("dir-sync");

    assert_eq!(
        directory_sync_supported(),
        cfg!(unix),
        "the support claim matches the compiled platform"
    );

    let mut storage = DirStorage::new(&base.0, PROFILE_PREFIX);
    storage
        .sync_dir()
        .expect("the production SyncDir phase never errors on this platform");

    // The raw probe, recorded for every platform: on unix a directory handle
    // opens and syncs; off unix `File::open` on a directory is expected to be
    // refused, which is *why* the phase is a no-op there.
    let raw = File::open(&base.0).and_then(|handle| handle.sync_all());
    if cfg!(unix) {
        assert!(
            raw.is_ok(),
            "a directory handle opens and syncs on unix: {raw:?}"
        );
    }
    println!("{}; raw dir-handle sync probe: {raw:?}", platform_note());
}
