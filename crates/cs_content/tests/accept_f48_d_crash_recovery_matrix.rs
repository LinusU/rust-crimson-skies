//! Acceptance scenarios F48-D: the crash/recovery matrix on the platform this
//! build actually runs on. Task test prefix: `accept_f48_d_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`, stage
//! `### F48-D`; contract `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence").
//!
//! **What is real here and what is modelled.** F48-B interrupted a commit by
//! returning an error from the [`SaveStorage`] seam, so the file operations that
//! ran were real but no process ever died. This file kills a *real* process: the
//! parent re-executes this same test binary with a request naming one write
//! phase, the child performs that phase's real file operations and then blocks,
//! and the parent kills it with [`std::process::Child::kill`] — `SIGKILL` on unix
//! and `TerminateProcess` on Windows, so no unwinding, no destructors and no
//! buffered flush happen in the child on any platform. That is a genuine process
//! death at a chosen point in the write.
//!
//! It is *not* a power cut. A kill cannot be: the bytes a process wrote are in
//! the page cache whether or not they were `fsync`ed, so this file cannot
//! discriminate a synced write from an unsynced one and does not claim to. What
//! it does establish is that no interleaving of process death with the phase
//! sequence loses or mixes a whole revision. Durability across a real power loss
//! needs hardware or a VM that can be cut, and this stage does not have either;
//! `docs/findings/2026-10-01-f48-d-crash-recovery-matrix.md` records that as
//! unmeasured rather than satisfied.
//!
//! A *torn* write (half a revision in the temp file) cannot be produced by a
//! kill either — a signal does not interrupt a `write` syscall in flight — so
//! the torn rows below are produced by truncating the file the way an
//! interrupted write would, and are labelled as that model rather than as a
//! measured crash.
//!
//! Every byte written here is newly authored synthetic data under the system
//! temporary directory, removed when the test finishes. No test touches a real
//! user profile directory, `$CS_GAME_DIR` or any original data.
//!
//! These tests call production code — `cs_content::save::fs::DirStorage`
//! through `store::commit`, `store::recover`, `library::ProfileLibrary` — so
//! removing the real-file mapping, the phase order, the selection rule or the
//! new slot-path and slot-identity refusals makes them fail.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cs_content::save::codec::{MAX_SAVE_BYTES, decode, encode};
use cs_content::save::fs::{
    DirStorage, directory_sync_supported, platform_note, replacement_semantics,
};
use cs_content::save::library::{ProfileLibrary, slot_name};
use cs_content::save::store::{
    PROFILE_PREFIX, RecoverError, RecoveryWarning, SaveFile, SavePhase, SaveStorage, StorageError,
    commit, recover,
};
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision};

/// A disposable population directory, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-d-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    /// A path below the base that nothing the library writes may touch, used to
    /// prove that a hostile slot cannot redirect a read or a write out of it.
    fn outside(&self) -> PathBuf {
        let path = self.0.join("outside-the-base");
        fs::create_dir_all(&path).expect("the canary directory");
        path
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A synthetic profile document with a money value that identifies its
/// revision, so a recovered revision can be checked whole rather than by field.
fn doc(id: ProfileId, revision: u64) -> ProfileDocument {
    let mut d = ProfileDocument::synthetic(id, Revision(revision));
    d.campaign.run_id = Some("run-a".into());
    d.campaign.money_minor = 1_000 + revision;
    d
}

/// The state a slot is in before the crash under test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreState {
    /// One revision installed, no backup.
    OneRevision,
    /// Two revisions installed: current and a backup.
    CurrentAndBackup,
    /// Current and backup installed, plus a *whole but uninstalled* newer
    /// revision in the temp file — what a kill in `SyncTemp` leaves behind.
    UninstalledNewerTemp,
}

impl PreState {
    const ALL: [Self; 3] = [
        Self::OneRevision,
        Self::CurrentAndBackup,
        Self::UninstalledNewerTemp,
    ];

    /// The newest whole revision a reader is entitled to see before the crash.
    fn expected_before(self) -> u64 {
        match self {
            Self::OneRevision => 1,
            Self::CurrentAndBackup => 2,
            Self::UninstalledNewerTemp => 3,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OneRevision => "one-revision",
            Self::CurrentAndBackup => "current+backup",
            Self::UninstalledNewerTemp => "newer-uninstalled-temp",
        }
    }
}

/// Where in a phase the process dies: before its effect reaches the filesystem,
/// or after it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Before,
    After,
}

impl Side {
    const ALL: [Self; 2] = [Self::Before, Self::After];

    fn label(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

/// One row of the matrix: a pre-state, a phase, a side, and what survived.
#[derive(Clone, Debug)]
struct Row {
    pre: PreState,
    phase: SavePhase,
    side: Side,
    /// The whole revision in force after the kill, if any.
    recovered: Option<u64>,
    /// Which file held it.
    source: Option<SaveFile>,
    /// The diagnostics recovery reported.
    warnings: Vec<RecoveryWarning>,
    /// How the child died, recorded so a row is evidence of a real death.
    death: Death,
    /// Whether the commit that followed the crash succeeded, and with which
    /// revision.
    recommitted: Option<u64>,
    /// Whether the file outside the base is untouched.
    canary_intact: bool,
}

// --- The crash harness ------------------------------------------------------

/// A real [`DirStorage`] that stops the process at one point of one phase.
///
/// The file operations are the production ones: `write_temp`, `sync_temp`,
/// `rotate_backup`, `install_current` and `sync_dir` are called on the real
/// [`DirStorage`]. What this adds is the place where the process stops, and a
/// marker the parent waits for before killing it.
struct CrashAt {
    inner: DirStorage,
    phase: SavePhase,
    side: Side,
    ready: PathBuf,
    /// Written half-length before the kill at `WriteTemp`, the way an
    /// interrupted write leaves a file. A signal cannot produce this itself.
    torn_write: bool,
    reached: bool,
}

impl CrashAt {
    /// Performs the real operation, then reports readiness and blocks, so the
    /// parent can kill this process while it is stopped exactly here.
    fn stop(&mut self) -> ! {
        fs::write(&self.ready, b"ready").expect("the ready marker is written");
        self.reached = true;
        // Blocked rather than returning: the parent kills this process while it
        // is here. No destructors run, no unwinding happens.
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn before(&self) -> bool {
        self.side == Side::Before
    }

    fn torn(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        if self.torn_write {
            let half = bytes.len() / 2;
            self.inner.write_temp(&bytes[..half])?;
        } else {
            self.inner.write_temp(bytes)?;
        }
        Ok(())
    }
}

impl SaveStorage for CrashAt {
    fn read(&self, file: SaveFile) -> Result<Option<Vec<u8>>, StorageError> {
        self.inner.read(file)
    }
    fn write_temp(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        if self.phase == SavePhase::WriteTemp {
            if self.before() {
                self.torn(bytes)?;
                self.stop();
            }
            self.torn(bytes)?;
            if self.side == Side::After {
                self.stop();
            }
            return Ok(());
        }
        self.inner.write_temp(bytes)
    }
    fn sync_temp(&mut self) -> Result<(), StorageError> {
        if self.phase == SavePhase::SyncTemp {
            if self.before() {
                self.stop();
            }
            self.inner.sync_temp()?;
            if self.side == Side::After {
                self.stop();
            }
            return Ok(());
        }
        self.inner.sync_temp()
    }
    fn rotate_backup(&mut self) -> Result<(), StorageError> {
        if self.phase == SavePhase::RotateBackup {
            if self.before() {
                self.stop();
            }
            self.inner.rotate_backup()?;
            if self.side == Side::After {
                self.stop();
            }
            return Ok(());
        }
        self.inner.rotate_backup()
    }
    fn install_current(&mut self) -> Result<(), StorageError> {
        if self.phase == SavePhase::InstallCurrent {
            if self.before() {
                self.stop();
            }
            self.inner.install_current()?;
            if self.side == Side::After {
                self.stop();
            }
            return Ok(());
        }
        self.inner.install_current()
    }
    fn sync_dir(&mut self) -> Result<(), StorageError> {
        if self.phase == SavePhase::SyncDir {
            if self.before() {
                self.stop();
            }
            self.inner.sync_dir()?;
            if self.side == Side::After {
                self.stop();
            }
            return Ok(());
        }
        self.inner.sync_dir()
    }
}

/// Everything the child is told to do, passed in the environment because the
/// child is this same test binary re-executed with a name filter.
struct CrashRequest {
    directory: PathBuf,
    id: u64,
    revision: u64,
    phase: SavePhase,
    side: Side,
    ready: PathBuf,
    torn: bool,
}

const ENV_PREFIX: &str = "CS_F48D_CRASH_";

impl CrashRequest {
    fn write_env(&self, command: &mut Command) {
        let pairs = [
            ("DIR", self.directory.to_string_lossy().into_owned()),
            ("ID", self.id.to_string()),
            ("REV", self.revision.to_string()),
            ("PHASE", format!("{:?}", self.phase)),
            ("SIDE", format!("{:?}", self.side)),
            ("READY", self.ready.to_string_lossy().into_owned()),
            ("TORN", self.torn.to_string()),
        ];
        for (key, value) in pairs {
            command.env(format!("{ENV_PREFIX}{key}"), value);
        }
    }
}

/// Spawns the child that will be killed, kills it at the point under test and
/// reaps it, returning the death the matrix recorded.
fn kill_at(request: &CrashRequest) -> Death {
    let exe = std::env::current_exe().expect("this test binary");
    let mut command = Command::new(exe);
    command
        .args([
            "--exact",
            CRASH_CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    request.write_env(&mut command);
    let mut child = command.spawn().expect("the crash child is spawned");

    // Wait for the child to report that it has performed the real operations up
    // to the point under test. Bounded, so a child that never arrives fails the
    // test instead of hanging it.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !request.ready.exists() {
        if let Some(status) = child.try_wait().expect("the child is waited for") {
            panic!("the crash child exited early with {status}: it never reached the crash point");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the crash child never reached its crash point");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // The real death: no unwinding, no destructors, no flush in the child.
    child.kill().expect("the child is killed");
    let status = child.wait().expect("the killed child is reaped");
    assert!(
        !status.success(),
        "the child must have died, not returned cleanly: {status}"
    );
    Death {
        killed_by_signal: status.code().is_none(),
        status: status.to_string(),
    }
}

/// How the killed child died, so a row records that it was a real death rather
/// than a clean return that only looked like one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Death {
    /// A signalled death reports no exit code on unix; a clean exit does.
    killed_by_signal: bool,
    status: String,
}

/// The name of the test that runs in the killed child.
const CRASH_CHILD: &str = "f48_d_crash_child_commits_and_dies_at_a_named_phase";

/// Runs one matrix row: build the pre-state, kill a real process at the phase,
/// reopen the slot and record what survived.
fn run_row(base: &TempBase, id: ProfileId, pre: PreState, phase: SavePhase, side: Side) -> Row {
    let directory = base.0.join(format!(
        "row-{}-{}-{}",
        pre.label(),
        phase_index(phase),
        side.label()
    ));
    let mut slot = DirStorage::create(&directory, PROFILE_PREFIX).expect("the slot");
    commit(&mut slot, &doc(id, 1)).expect("the first revision");
    if pre != PreState::OneRevision {
        commit(
            &mut DirStorage::new(&directory, PROFILE_PREFIX),
            &doc(id, 2),
        )
        .expect("the second revision");
    }
    if pre == PreState::UninstalledNewerTemp {
        // What a kill in `SyncTemp` leaves: the whole next revision written and
        // synced but never installed.
        let bytes = encode(&doc(id, 3)).expect("the uninstalled revision encodes");
        fs::write(directory.join("profile.tmp"), bytes).expect("the uninstalled temp");
    }
    let before = pre.expected_before();
    // The commit under test writes the revision after whatever is installed.
    let mut revision = before;
    revision += 1;
    if pre == PreState::UninstalledNewerTemp {
        revision = 4;
    }

    let canary = base.outside();
    fs::write(canary.join("canary"), b"not the slot's business").expect("the canary file");
    let canary_before = fs::read(canary.join("canary")).expect("the canary");

    let ready = base.0.join(format!(
        "ready-{:?}-{}-{}",
        phase,
        pre.label(),
        side.label()
    ));
    let _ = fs::remove_file(&ready);
    let request = CrashRequest {
        directory: directory.clone(),
        id: id.get(),
        revision,
        phase,
        side,
        ready: ready.clone(),
        torn: phase == SavePhase::WriteTemp && side == Side::Before,
    };
    let death = kill_at(&request);

    // Reopen through the production recovery.
    let reopened = DirStorage::new(&directory, PROFILE_PREFIX);
    let (recovered, source, warnings) = match recover(&reopened) {
        Ok(found) => found.map_or((None, None, Vec::new()), |found| {
            (
                Some(found.document.revision.0),
                Some(found.source),
                found.warnings,
            )
        }),
        // A refusal is an outcome of the matrix, not a failure of it: the row
        // records it and the caller decides whether it is acceptable.
        Err(RecoverError::NoValidSave { diagnostics }) => {
            let mut warnings = diagnostics
                .into_iter()
                .map(|(file, error)| RecoveryWarning::Corrupt { file, error })
                .collect::<Vec<_>>();
            warnings.sort_by_key(|warning| match warning {
                RecoveryWarning::Corrupt { file, .. } => *file,
                RecoveryWarning::UsedFallback { source } => *source,
            });
            (None, None, warnings)
        }
        Err(other) => panic!(
            "reopening after the crash at {phase:?}/{}: {other}",
            side.label()
        ),
    };

    // The commit that follows the crash must succeed and keep the whole
    // revision that was in force before it.
    let next = revision + 1;
    let recommitted = match commit(
        &mut DirStorage::new(&directory, PROFILE_PREFIX),
        &doc(id, next),
    ) {
        Ok(()) => {
            let found = recover(&reopened)
                .expect("the slot after the recommit")
                .expect("a whole revision after the recommit");
            assert_eq!(
                found.document.revision,
                Revision(next),
                "the recommit installed exactly the revision offered"
            );
            Some(found.document.revision.0)
        }
        Err(error) => panic!(
            "the commit after the crash at {phase:?}/{}: {error}",
            side.label()
        ),
    };

    Row {
        pre,
        phase,
        side,
        recovered,
        source,
        warnings,
        death,
        recommitted,
        canary_intact: fs::read(canary.join("canary")).expect("the canary") == canary_before,
    }
}

/// A stable index per phase, so each row gets its own directory.
fn phase_index(phase: SavePhase) -> usize {
    SavePhase::ALL
        .iter()
        .position(|candidate| *candidate == phase)
        .expect("the phase is one of the five")
}

/// The whole matrix: every phase, both sides of it, in every pre-state.
fn run_matrix(base: &TempBase, id: ProfileId) -> Vec<Row> {
    let mut rows = Vec::new();
    for pre in PreState::ALL {
        for phase in SavePhase::ALL {
            for side in Side::ALL {
                rows.push(run_row(base, id, pre, phase, side));
            }
        }
    }
    rows
}

// --- The tests --------------------------------------------------------------

/// The crash/recovery matrix: a real process killed before and after every
/// phase of the write sequence, in every pre-state, must leave a whole revision
/// a reader can use — never a mixture of two files and never nothing.
#[test]
fn accept_f48_d_a_kill_at_every_phase_boundary_leaves_a_whole_revision() {
    let base = TempBase::new("kill-matrix");
    let id = ProfileId::new(1).expect("nonzero");
    let rows = run_matrix(&base, id);
    assert_eq!(
        rows.len(),
        PreState::ALL.len() * SavePhase::ALL.len() * Side::ALL.len(),
        "the matrix covers every pre-state, phase and side"
    );

    for row in &rows {
        let context = format!("{}/{}/{:?}", row.pre.label(), row.side.label(), row.phase);
        assert!(
            row.death.killed_by_signal,
            "{context}: the child died from a signal, not a clean exit ({})",
            row.death.status
        );
        assert!(
            row.canary_intact,
            "{context}: nothing outside the base was touched"
        );
        let recovered = row
            .recovered
            .unwrap_or_else(|| panic!("{context}: a whole revision survives a real process death"));
        let floor = row.pre.expected_before();
        // The revision in force is the newest whole revision that was written,
        // and never one that was not: the offered revision (or the uninstalled
        // temp, if the phase never reached the install) is the upper bound.
        let ceiling = floor + 1;
        assert!(
            recovered == floor || recovered == ceiling,
            "{context}: recovered revision {recovered} is neither the pre-crash \
             whole revision {floor} nor the one offered ({ceiling})"
        );
        // A revision that is not the current file must say so.
        if row.source != Some(SaveFile::Current) {
            assert!(
                row.warnings
                    .iter()
                    .any(|warning| matches!(warning, RecoveryWarning::UsedFallback { .. })),
                "{context}: a fallback is reported, not silent: {:?}",
                row.warnings
            );
        }
        assert_eq!(
            row.recommitted,
            Some(ceiling + 1),
            "{context}: the commit after the crash succeeded"
        );
    }
}

/// A slot whose every file was destroyed or unreadable is reported rather than
/// silently presented as a profile with no state, and it is writable again.
#[test]
fn accept_f48_d_a_slot_with_no_recoverable_revision_is_reported_not_hidden() {
    let base = TempBase::new("no-recovery");
    let id = ProfileId::new(1).expect("nonzero");
    let directory = base.0.join(slot_name(id));
    let mut slot = DirStorage::create(&directory, PROFILE_PREFIX).expect("the slot");
    commit(&mut slot, &doc(id, 1)).expect("first");

    for name in ["profile.sav", "profile.bak", "profile.tmp"] {
        let _ = fs::remove_file(directory.join(name));
        fs::write(directory.join(name), b"destroyed").expect("damage");
    }
    let err = recover(&DirStorage::new(&directory, PROFILE_PREFIX))
        .expect_err("nothing in the slot is readable");
    let text = err.to_string();
    assert!(
        text.contains("3") || text.contains("three") || text.contains("unreadable"),
        "the refusal says how many files were unreadable: {text}"
    );
    // Every damaged file is kept, so an owner can look at what happened.
    for name in ["profile.sav", "profile.bak", "profile.tmp"] {
        assert_eq!(
            fs::read(directory.join(name)).expect("kept"),
            b"destroyed",
            "{name} is kept, not deleted"
        );
    }
    // And the slot is writable again rather than bricked.
    commit(
        &mut DirStorage::new(&directory, PROFILE_PREFIX),
        &doc(id, 5),
    )
    .expect("the slot is writable again");
}

/// The matrix is only worth having if it actually runs: this records what the
/// platform under test is, asserts the reported facts match the compiled
/// platform rather than a claim, and prints the matrix so the run log carries it.
#[test]
fn accept_f48_d_the_matrix_runs_and_reports_this_platform() {
    let base = TempBase::new("platform");
    let id = ProfileId::new(1).expect("nonzero");

    // The replacement call this build uses is the one the platform's std
    // actually makes, and the directory sync claim is a platform fact.
    let expected = if cfg!(windows) {
        "MoveFileExW"
    } else {
        "rename(2)"
    };
    assert_eq!(
        replacement_semantics().to_string(),
        expected,
        "the replacement call matches the compiled platform"
    );
    assert_eq!(
        directory_sync_supported(),
        cfg!(unix),
        "directory sync is supported exactly where it is"
    );
    let note = platform_note();
    assert!(
        note.contains(expected),
        "the platform note names it: {note}"
    );
    assert!(
        note.contains("supported") || note.contains("no-op"),
        "the platform note states whether a directory sync does anything: {note}"
    );

    let rows = run_matrix(&base, id);
    let survived = rows.iter().filter(|row| row.recovered.is_some()).count();
    assert_eq!(
        survived,
        rows.len(),
        "every row of the matrix recovered a whole revision"
    );
    let from_current = rows
        .iter()
        .filter(|row| row.source == Some(SaveFile::Current))
        .count();
    let fallbacks = rows.len() - from_current;
    println!("{note}");
    println!(
        "F48-D crash/recovery matrix on {}: {} rows, {from_current} recovered from \
         profile.sav, {fallbacks} through a reported fallback, 0 lost",
        std::env::consts::OS,
        rows.len()
    );
    for row in &rows {
        println!(
            "  {:<24} {:<16} {:<6} -> {:?} from {:?}{}",
            row.pre.label(),
            format!("{:?}", row.phase),
            row.side.label(),
            row.recovered,
            row.source,
            if row.warnings.is_empty() {
                String::new()
            } else {
                format!(
                    " [{}]",
                    row.warnings
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }
        );
    }
}

/// AC04 on the crash path: a save left behind by a killed process is recovered
/// or refused, and in every case a whole revision is still there for the player.
#[test]
fn accept_f48_d_a_killed_commit_never_leaves_the_slot_unrecoverable() {
    let base = TempBase::new("killed-commit");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let (id, _) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    // Three installed revisions, so current and backup are both populated.
    for revision in 2..=3 {
        let mut document = doc(id, revision);
        library.save(&document).expect("save");
        document.revision = Revision(revision);
    }
    drop(library);

    let ready = base.0.join("ready");
    let _ = fs::remove_file(&ready);
    let request = CrashRequest {
        directory: population.join(slot_name(id)),
        id: id.get(),
        revision: 4,
        phase: SavePhase::InstallCurrent,
        side: Side::Before,
        ready: ready.clone(),
        torn: false,
    };
    let death = kill_at(&request);
    assert!(
        death.killed_by_signal,
        "the profile crash child died from a signal ({})",
        death.status
    );

    // A fresh library over the same directory: what a player gets on the next
    // launch after a crash.
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("reopen");
    let loaded = library
        .load(id)
        .expect("the profile is loadable after a crash");
    let document = loaded.document.as_ref().expect("a whole revision");
    assert!(
        document.revision == Revision(3) || document.revision == Revision(4),
        "the whole revision before or after the crash, not a mixture: {:?}",
        document.revision
    );
    assert_eq!(document, &doc(id, document.revision.0));

    // And the session can carry on: the next commit succeeds.
    library
        .save(&ProfileDocument {
            revision: document.revision.next().expect("a successor"),
            ..document.clone()
        })
        .expect("the profile is saveable after a crash");
}

/// An unreadable *registry* after a crash is recovered from the slots, so no id
/// is issued twice — the property that makes a profile id an identity.
#[test]
fn accept_f48_d_a_killed_registry_write_never_reissues_a_profile_id() {
    let base = TempBase::new("killed-registry");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let mut ids = Vec::new();
    for expected in 1..=3 {
        let (id, _) = library
            .create(doc(ProfileId::new(expected).expect("nonzero"), 1))
            .expect("create");
        assert_eq!(id.get(), expected, "ids are allocated from the mark");
        ids.push(id);
    }
    // The newest registry is destroyed, as a torn write across a power cut would
    // leave it; the backup is the one an older commit wrote.
    let newest = population.join("registry.sav");
    let mut damaged = fs::read(&newest).expect("the newest registry");
    damaged[20] ^= 0x40;
    fs::write(&newest, damaged).expect("the newest registry is damaged");
    drop(library);

    let reopened = ProfileLibrary::open(population.clone(), kind).expect("reopen");
    assert!(
        !reopened.status().registry_warnings.is_empty(),
        "the damaged newest registry is reported"
    );
    assert!(
        reopened.status().high_water >= 3,
        "the mark in force still covers every id ever issued: {}",
        reopened.status().high_water
    );
    // Creating again must move past every id that was ever issued, not reuse
    // the one whose registry entry the damaged file lost.
    let mut reopened = reopened;
    let (id, _) = reopened
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create after the damaged registry");
    assert!(
        id.get() > ids.iter().map(|issued| issued.get()).max().expect("ids"),
        "a damaged newest registry did not reissue an id: {id}"
    );
}

/// The crash child: it is only ever run as the process this file kills, so it is
/// `#[ignore]`d and named without the task prefix. It performs the real commit
/// through the production path and stops at the named point.
#[test]
#[ignore = "the crash child, spawned and killed by the matrix in this file"]
fn f48_d_crash_child_commits_and_dies_at_a_named_phase() {
    let read = |key: &str| std::env::var(format!("{ENV_PREFIX}{key}")).ok();
    let (Some(directory), Some(id), Some(revision), Some(phase), Some(side)) = (
        read("DIR"),
        read("ID"),
        read("REV"),
        read("PHASE"),
        read("SIDE"),
    ) else {
        // Run directly by a person rather than spawned: there is no request to
        // honour, so say so instead of pretending to be a matrix row.
        println!("no crash request in the environment; nothing to do");
        return;
    };
    let phase = match phase.as_str() {
        "WriteTemp" => SavePhase::WriteTemp,
        "SyncTemp" => SavePhase::SyncTemp,
        "RotateBackup" => SavePhase::RotateBackup,
        "InstallCurrent" => SavePhase::InstallCurrent,
        "SyncDir" => SavePhase::SyncDir,
        other => panic!("unknown phase {other}"),
    };
    let side = match side.as_str() {
        "Before" => Side::Before,
        "After" => Side::After,
        other => panic!("unknown side {other}"),
    };
    let id = ProfileId::new(id.parse().expect("an id in the request")).expect("nonzero");
    let mut storage = CrashAt {
        inner: DirStorage::new(&directory, PROFILE_PREFIX),
        phase,
        side,
        ready: PathBuf::from(read("READY").expect("the ready marker path")),
        torn_write: read("TORN").as_deref() == Some("true"),
        reached: false,
    };
    // The production commit, through the real file operations, stopping at the
    // named point. If the stop is never reached the process exits normally and
    // the parent fails the row rather than killing something harmless.
    let outcome = commit(
        &mut storage,
        &doc(id, revision.parse().expect("a revision")),
    );
    assert!(
        !storage.reached,
        "the commit returned {outcome:?} without reaching its crash point"
    );
    unreachable!("the crash point blocks and the parent kills this process");
}

// --- The hostile-save half of this stage -----------------------------------

/// A registry or a save that names a slot outside its own population is refused,
/// so no read or write can be redirected out of the base directory.
#[test]
fn accept_f48_d_a_hostile_slot_path_is_refused_and_nothing_outside_is_written() {
    let base = TempBase::new("hostile-slot-path");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let (id, _) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    drop(library);

    let canary = base.outside();
    let held = b"the real file behind the planted link";
    fs::write(canary.join("canary.sav"), held).expect("the canary save");
    let slot = population.join(slot_name(id));
    let moved = population.join("moved-out-of-the-way");
    fs::rename(&slot, &moved).expect("the slot is moved aside");
    // The canary is a complete, valid save of the same profile, so a read that
    // followed the link would succeed and return it. It must not be followed.
    let mut canary_slot = DirStorage::create(&canary, PROFILE_PREFIX).expect("canary slot");
    commit(&mut canary_slot, &doc(id, 99)).expect("the canary save");
    drop(canary_slot);
    let canary_before = fs::read(canary.join("profile.sav")).expect("the canary bytes");

    #[cfg(unix)]
    if std::os::unix::fs::symlink(&canary, &slot).is_ok() {
        let mut library = ProfileLibrary::open(population.clone(), kind).expect("reopen");
        let err = library
            .load(id)
            .expect_err("a symbolic link at the slot path is refused");
        let text = err.to_string();
        assert!(
            text.contains("not a directory"),
            "the refusal names the slot path: {text}"
        );
        // Nothing was read through it and nothing was written to it.
        assert_eq!(
            library.live(),
            &[id],
            "a planted link is not adopted as a profile"
        );
        assert_eq!(
            fs::read(canary.join("profile.sav")).expect("the canary bytes"),
            canary_before,
            "the file outside the base was neither read nor written through the link"
        );
        let report = library
            .slot_warnings(id)
            .expect_err("the diagnostics path refuses it too");
        assert!(
            report.to_string().contains("not a directory"),
            "slot_warnings refuses the same path: {report}"
        );
        assert!(
            matches!(
                library.save(&doc(id, 2)),
                Err(cs_content::save::library::LibraryError::Slot(
                    cs_content::save::fs::SlotError::NotADirectory(_)
                ))
            ),
            "and the write path refuses it as well: {:?}",
            library.save(&doc(id, 2)).map_err(|error| error.to_string())
        );
    }

    // A *regular file* where the slot directory belongs is refused everywhere,
    // with or without symbolic links.
    let plain = base.0.join("plain");
    let file_slot = plain.join("profile-999");
    fs::create_dir_all(&plain).expect("the plain population");
    fs::write(&file_slot, b"not a directory").expect("a file at the slot path");
    let library = ProfileLibrary::open(plain, kind).expect("open");
    assert!(
        library
            .load(ProfileId::new(999).expect("nonzero"))
            .expect_err("a file at the slot path is refused")
            .to_string()
            .contains("not a directory"),
        "a regular file at the slot path is refused"
    );
    assert!(
        library
            .slot_warnings(ProfileId::new(999).expect("nonzero"))
            .is_err(),
        "the diagnostics path refuses it too"
    );
}

/// A slot holding a *different* profile's save is refused, so an id never comes
/// to mean another pilot's campaign, records and settings.
#[test]
fn accept_f48_d_a_slot_holding_another_profile_is_refused() {
    let base = TempBase::new("foreign-slot");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let (first, _) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    let (second, mut second_document) = library
        .create(doc(ProfileId::new(2).expect("nonzero"), 1))
        .expect("create");
    second_document.revision = Revision(2);
    second_document.campaign.money_minor = 4_242;
    library.save(&second_document).expect("save");

    // The first profile's slot now holds the second profile's save, as a
    // misplaced file or a careless copy would leave it.
    let slot = library.slot_dir(first);
    for file in [SaveFile::Current, SaveFile::Backup, SaveFile::Temp] {
        let _ = fs::remove_file(slot.join(file.prefixed_name(PROFILE_PREFIX)));
    }
    fs::copy(
        library
            .slot_dir(second)
            .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)),
        slot.join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)),
    )
    .expect("the misplaced save is planted");

    let err = library
        .load(first)
        .expect_err("another profile's save is refused");
    let text = err.to_string();
    assert!(
        text.contains(&first.to_string()) && text.contains(&second.to_string()),
        "the refusal names both profiles: {text}"
    );
    // The planted bytes are left exactly as they were found.
    assert_eq!(
        decode(
            &fs::read(slot.join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)))
                .expect("the planted save")
        )
        .expect("it is still a whole save")
        .profile_id,
        second,
        "the other profile's bytes are untouched"
    );
    // And the second profile is unaffected.
    let loaded = library
        .load(second)
        .expect("the second profile still loads");
    assert_eq!(
        loaded.document.as_ref().expect("a revision").revision,
        Revision(2)
    );
}

/// A slot holding a document from *another population* — a synthetic directory
/// with a production pilot's save planted in it, under the synthetic id — is
/// refused, so a synthetic or evidence library never reads production state
/// through a misplaced file (F48 non-negotiable 4). The id check alone would
/// accept this: the identity that separates the populations is the document
/// kind, which `create`/`save` already enforce on the write path.
#[test]
fn accept_f48_d_a_slot_holding_another_population_is_refused() {
    let base = TempBase::new("foreign-population");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let (id, _) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");

    // The same id, but a *production* document, as a careless copy or a planted
    // file would leave it.
    let mut foreign = doc(id, 2);
    foreign.kind = ProfileKind::Production;
    foreign.campaign.money_minor = 4_242;
    let bytes = encode(&foreign).expect("the foreign document encodes");
    let slot = library.slot_dir(id);
    for file in [SaveFile::Current, SaveFile::Backup, SaveFile::Temp] {
        let _ = fs::remove_file(slot.join(file.prefixed_name(PROFILE_PREFIX)));
    }
    fs::write(
        slot.join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)),
        bytes.clone(),
    )
    .expect("the foreign save is planted");

    let err = library
        .load(id)
        .expect_err("another population's save is refused");
    assert!(
        matches!(
            err,
            cs_content::save::library::LibraryError::ForeignDocument { id: refused, .. }
                if refused == id
        ),
        "the refusal is the population refusal: {err:?}"
    );
    // The planted bytes are left exactly as they were found.
    assert_eq!(
        fs::read(slot.join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)))
            .expect("the planted save"),
        bytes,
        "the other population's bytes are untouched"
    );
}

/// The oversized case at the recovery level the matrix cares about: a file over
/// the bound is refused by the length it reports, never read in full, and never
/// overwritten by a commit.
#[test]
fn accept_f48_d_an_oversized_save_is_refused_before_it_is_read_and_never_overwritten() {
    let base = TempBase::new("oversized");
    let id = ProfileId::new(1).expect("nonzero");
    let directory = base.0.join(slot_name(id));
    let mut slot = DirStorage::create(&directory, PROFILE_PREFIX).expect("slot");
    commit(&mut slot, &doc(id, 1)).expect("first");
    commit(
        &mut DirStorage::new(&directory, PROFILE_PREFIX),
        &doc(id, 2),
    )
    .expect("second");

    let oversized = vec![b'a'; MAX_SAVE_BYTES + 1];
    fs::write(directory.join("profile.sav"), &oversized).expect("write");

    let err = recover(&DirStorage::new(&directory, PROFILE_PREFIX)).expect_err("oversized");
    let text = err.to_string();
    assert!(
        text.contains(&format!("{MAX_SAVE_BYTES}")),
        "the refusal names the bound: {text}"
    );
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        oversized,
        "the oversized bytes are not overwritten by a commit"
    );
    assert!(
        commit(
            &mut DirStorage::new(&directory, PROFILE_PREFIX),
            &doc(id, 3)
        )
        .is_err(),
        "a commit over an oversized current file is refused rather than writing over it"
    );
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        oversized
    );
    // The whole backup is still there for the player, and readable directly.
    let backup = fs::read(directory.join("profile.bak")).expect("the backup");
    assert_eq!(
        decode(&backup).expect("the backup is whole").revision,
        Revision(1)
    );
    assert!(
        commit(
            &mut DirStorage::new(&directory, PROFILE_PREFIX),
            &doc(id, 3)
        )
        .is_err(),
        "still refused"
    );
}

/// A path at a save name that is not a bounded regular file cannot make the read
/// run without limit: `/dev/zero` reports no length and reads forever, so the
/// read has to be capped for this test to finish at all.
#[cfg(unix)]
#[test]
fn accept_f48_d_a_device_at_a_save_name_is_refused_at_the_bound_not_after_reading_it() {
    let base = TempBase::new("device-at-save-name");
    let id = ProfileId::new(1).expect("nonzero");
    let directory = base.0.join(slot_name(id));
    DirStorage::create(&directory, PROFILE_PREFIX).expect("the slot");
    std::os::unix::fs::symlink("/dev/zero", directory.join("profile.sav"))
        .expect("a device at the save name");

    let err =
        recover(&DirStorage::new(&directory, PROFILE_PREFIX)).expect_err("a device is not a save");
    let text = err.to_string();
    assert!(
        text.contains("over the") && text.contains(&format!("{MAX_SAVE_BYTES}")),
        "the refusal is the bound, reported before the read: {text}"
    );
}

/// A future-schema save is refused and its bytes are preserved, including
/// through the library's commit path — the property that lets a player go back
/// to the build that wrote it.
#[test]
fn accept_f48_d_a_future_schema_save_is_refused_and_preserved() {
    let base = TempBase::new("future");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("open");
    let (id, mut document) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    document.revision = Revision(2);
    library.save(&document).expect("save");

    let directory = library.slot_dir(id);
    // A whole, checksummed save from a build whose schema is newer than this
    // one's. Sealed so the refusal is about the version and not about framing.
    use cs_content::save::codec::checksum;
    let body = "CSSAVE 9.0\nprofile_id=1\nkind=synthetic\ndisplay_name=Future\nrevision=9\ncampaign.money_minor=1\n";
    let future = format!("{body}checksum={:016x}\n", checksum(body.as_bytes())).into_bytes();
    fs::write(directory.join("profile.sav"), future.clone()).expect("write");

    let err = library.load(id).expect_err("a future save is refused");
    assert!(
        matches!(
            err,
            cs_content::save::library::LibraryError::Recover(RecoverError::UnsupportedMajor { .. })
        ),
        "the refusal is the version refusal: {err:?}"
    );
    assert!(
        library.save(&doc(id, 3)).is_err(),
        "a future save is never overwritten by this build"
    );
    assert_eq!(
        fs::read(directory.join("profile.sav")).expect("still there"),
        future,
        "the newer build's bytes are untouched"
    );
    // The backup the older build wrote is still readable and intact.
    let backup = fs::read(directory.join("profile.bak")).expect("the backup");
    assert_eq!(
        decode(&backup).expect("the backup is whole").revision,
        Revision(1)
    );
}

/// A slot directory that holds no save file at all is not a profile: a crash
/// between creating the directory and writing the first revision must not
/// produce a pilot with nothing to load.
#[test]
fn accept_f48_d_an_empty_slot_directory_is_not_adopted_as_a_profile() {
    let base = TempBase::new("empty-slot");
    let kind = ProfileKind::Synthetic;
    let population = base.0.join(kind.label());
    let mut library = ProfileLibrary::open(population.clone(), kind).expect("create some");
    let (id, _) = library
        .create(doc(ProfileId::new(1).expect("nonzero"), 1))
        .expect("create");
    // The registry is destroyed, and the slot is emptied: what a crash in the
    // middle of the very first write leaves behind.
    for name in ["registry.sav", "registry.bak", "registry.tmp"] {
        let _ = fs::remove_file(population.join(name));
    }
    let slot = population.join(slot_name(id));
    for name in ["profile.sav", "profile.bak", "profile.tmp"] {
        let _ = fs::remove_file(slot.join(name));
    }
    drop(library);

    let library = ProfileLibrary::open(population, kind).expect("reopen");
    assert!(
        library.live().is_empty(),
        "a slot with no save file is not a profile: {:?}",
        library.live()
    );
    assert!(
        library.status().high_water >= id.get(),
        "the id it names still raises the mark, so it is never issued again: {}",
        library.status().high_water
    );
    let loaded = library.load(id).expect("the empty slot loads as empty");
    assert!(
        loaded.document.is_none(),
        "and it carries no document rather than a default one"
    );
}
