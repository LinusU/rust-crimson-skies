//! Acceptance scenarios F48-D at the *runtime* level: a hostile save arriving
//! through the session that a run actually holds, after a real process death.
//! Task test prefix: `accept_f48_d_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`, stage
//! `### F48-D`, acceptance case AC04 ("load a future/oversized/malicious save
//! without panic, traversal or destructive overwrite"); contract
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Session reset", "Persistence").
//!
//! The companion test `crates/cs_content/tests/accept_f48_d_crash_recovery_matrix.rs`
//! runs the same matrix against the storage layer directly. This file asks the
//! question the storage layer cannot: what does a *session* do when the save it
//! is handed is hostile — does it open, does it report, and does anything
//! outside the population directory get touched.
//!
//! Every profile here is newly authored synthetic data in a temporary
//! directory, removed on drop. No test opens a real user's profile tree,
//! `$CS_GAME_DIR` or any original data. The settings keys come from a catalog
//! this file declares and are engine vocabulary, not claims about an original
//! setting.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cs_app::profile::{ProfileSession, SessionOrigin};
use cs_content::save::codec::{MAX_SAVE_BYTES, checksum};
use cs_content::save::fs::DirStorage;
use cs_content::save::library::{ProfileLibrary, load_profile_slot, slot_name};

use cs_content::save::settings::{SettingCatalog, SettingRule, ValueRule};
use cs_content::save::store::{PROFILE_PREFIX, RecoverError, SaveFile, SaveStorage, commit};
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision, SettingApply};

/// A disposable user-data base, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-d-app-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// A path outside the base that nothing may touch.
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

/// The catalog this file declares. Nothing here is an original-game setting.
fn catalog() -> SettingCatalog {
    SettingCatalog::new([
        SettingRule {
            key: "display.mode",
            apply: SettingApply::RestartRequired,
            value: ValueRule::Choice(&["window=ed", "full-screen=ed"]),
            default: "window=ed",
        },
        SettingRule {
            key: "audio.volume",
            apply: SettingApply::Live,
            value: ValueRule::Integer { min: 0, max: 10 },
            default: "8",
        },
    ])
    .expect("the fixture catalog holds together")
}

fn synthetic(id: ProfileId, revision: u64) -> ProfileDocument {
    let mut document = ProfileDocument::synthetic(id, Revision(revision));
    document.campaign.run_id = Some("run-a".into());
    document.campaign.money_minor = 1_000 + revision;
    document
}

/// A sealed save of a newer schema than this build reads, checksummed so the
/// refusal under test is the version and not the framing.
fn future_save(id: ProfileId, major: u16) -> Vec<u8> {
    let body = format!(
        "CSSAVE {major}.0\nprofile_id={id}\nkind=synthetic\ndisplay_name=Future\n\
         revision=9\ncampaign.money_minor=1\n"
    );
    format!("{body}checksum={:016x}\n", checksum(body.as_bytes())).into_bytes()
}

/// A session over the synthetic population, the way automation opens one.
fn sandbox(base: &Path) -> ProfileSession {
    ProfileSession::open_sandbox(base, &catalog()).expect("the sandbox session opens")
}

// --- AC04 through the session ----------------------------------------------

/// A future-schema save is refused at open, the session still opens so the other
/// pilots remain reachable, and the newer build's bytes are never overwritten.
#[test]
fn accept_f48_d_a_future_save_does_not_stop_the_session_and_is_never_overwritten() {
    let base = TempBase::new("future");
    {
        let mut session = sandbox(base.path());
        let id = session.create("First").expect("create");
        // A second profile, so the failure under test cannot be mistaken for "the
        // population has nothing else in it".
        session.create("Second").expect("create");
        session.select(id).expect("select");
        let mut document = session.document().expect("a document").clone();
        document.revision = Revision(2);
        document.campaign.money_minor = 1_002;
        session.commit().expect("second revision");

        let population = base.path().join(ProfileKind::Synthetic.label());
        fs::write(
            population
                .join(slot_name(id))
                .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)),
            future_save(id, 9),
        )
        .expect("the future save is planted");
    }

    // A new session over the same population: the active pointer names the
    // damaged profile, so this is the path that has to degrade rather than fail.
    let mut session = sandbox(base.path());
    let population = base.path().join(ProfileKind::Synthetic.label());
    let planted = future_save(ProfileId::new(1).expect("nonzero"), 9);
    assert_eq!(
        fs::read(
            population
                .join(slot_name(ProfileId::new(1).expect("nonzero")))
                .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX))
        )
        .expect("still there"),
        planted,
        "the newer build's bytes are untouched by opening"
    );
    // The session opened, and says why it selected nothing.
    let warnings = session.warnings().join(" | ");
    assert!(
        warnings.contains("9.0") || warnings.contains("unsupported schema"),
        "the refusal is reported as text a caller can show: {warnings}"
    );
    assert_eq!(
        session.selected(),
        None,
        "a profile whose save cannot be read is not selected"
    );
    // The other pilot is still selectable, so one damaged save does not hide
    // the whole population.
    assert_eq!(session.live().len(), 2, "both pilots are listed");
    session
        .select(ProfileId::new(2).expect("nonzero"))
        .expect("the undamaged pilot is selectable");
    assert_eq!(session.document().expect("a document").profile_id.get(), 2);
    // And the damaged profile can still be selected-then-refused rather than
    // panicking, and its future bytes are still intact afterwards.
    assert!(
        session.select(ProfileId::new(1).expect("nonzero")).is_err(),
        "selecting the damaged profile is refused"
    );
    assert_eq!(
        fs::read(
            population
                .join(slot_name(ProfileId::new(1).expect("nonzero")))
                .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX))
        )
        .expect("still there"),
        planted
    );
}

/// An oversized file is refused at the bound, the session reports it and can
/// still carry on with another pilot, and the oversized bytes are not destroyed.
#[test]
fn accept_f48_d_an_oversized_save_is_refused_without_panicking_or_overwriting() {
    let base = TempBase::new("oversized");
    let population = base.path().join(ProfileKind::Synthetic.label());
    let oversized = vec![b'a'; MAX_SAVE_BYTES + 1];
    {
        let mut session = sandbox(base.path());
        session.create("First").expect("create");
        session.create("Second").expect("create");
        // The pointer names the profile whose save is damaged below, so the
        // session really does read it on the way in rather than selecting the
        // other pilot and never seeing the damage.
        session
            .select(ProfileId::new(1).expect("nonzero"))
            .expect("select the profile that will be damaged");
    }
    fs::write(
        population
            .join(slot_name(ProfileId::new(1).expect("nonzero")))
            .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX)),
        &oversized,
    )
    .expect("the oversized save is planted");

    let mut session = sandbox(base.path());
    let warnings = session.warnings().join(" | ");
    assert!(
        warnings.contains(&format!("{MAX_SAVE_BYTES}")) || warnings.contains("over the"),
        "the refusal names the bound: {warnings}"
    );
    assert_eq!(
        session.selected(),
        None,
        "an oversized save is not selected"
    );
    session
        .select(ProfileId::new(2).expect("nonzero"))
        .expect("the other pilot is selectable");
    assert_eq!(
        fs::read(
            population
                .join(slot_name(ProfileId::new(1).expect("nonzero")))
                .join(SaveFile::Current.prefixed_name(PROFILE_PREFIX))
        )
        .expect("still there"),
        oversized,
        "the oversized bytes were not overwritten"
    );
}

/// A malicious save whose fields name paths — a `campaign.run` or an unknown key
/// spelled as a traversal, a display name holding path separators — is refused
/// or carried as data, and in no case is anything written outside the profile's
/// own slot directory.
#[test]
fn accept_f48_d_a_save_naming_paths_writes_nothing_outside_its_slot() {
    let base = TempBase::new("traversal");
    let population = base.path().join(ProfileKind::Synthetic.label());
    let canary = base.outside();
    fs::write(canary.join("canary"), b"untouched").expect("the canary file");
    let id = ProfileId::new(1).expect("nonzero");

    // A save whose every field is an attempt to name a path. Sealed with a valid
    // checksum, so the decoder accepts the framing and the *bounds* are what
    // refuse the values.
    let body = format!(
        "CSSAVE 1.0\nprofile_id={id}\nkind=synthetic\nrevision=1\n\
         campaign.money_minor=0\n\
         campaign.run=../../../outside-the-base\n\
         display_name=../../escape\n\
         setting.display.mode=live:../../../outside-the-base\n\
         unknown_key=/etc/passwd\n\
         fingerprint.catalog=0123456789abcdef\n"
    );
    let malicious = format!("{body}checksum={:016x}\n", checksum(body.as_bytes())).into_bytes();

    // Planted as the whole slot, so recovery has to deal with it.
    let slot = DirStorage::create(population.join(slot_name(id)), PROFILE_PREFIX)
        .expect("the slot directory");
    fs::write(slot.path(SaveFile::Current), malicious.clone())
        .expect("the malicious save is planted");

    // The library refuses it on the field bounds, and the bytes survive.
    let library = ProfileLibrary::open(population.clone(), ProfileKind::Synthetic)
        .expect("the population opens");
    let err = library
        .load(id)
        .expect_err("a key that traverses is refused");
    let diagnostics = match &err {
        cs_content::save::library::LibraryError::Recover(RecoverError::NoValidSave {
            diagnostics,
        }) => diagnostics.clone(),
        other => panic!("the whole slot is unreadable, not one file: {other:?}"),
    };
    let named = diagnostics
        .iter()
        .map(|(file, error)| format!("{}: {error}", file.file_name()))
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        named.contains("disallowed character"),
        "the refusal names the character rule and the file it was in: {named}"
    );
    assert!(
        named.contains("profile.sav"),
        "and the file it was in: {named}"
    );
    assert!(
        err.to_string().contains("unreadable"),
        "the top-level refusal counts the unreadable files rather than staying silent: {err}"
    );
    assert_eq!(
        fs::read(slot.path(SaveFile::Current)).expect("still there"),
        malicious,
        "the refused bytes are kept, not rewritten"
    );

    // A display name is free text, so a player may legitimately call a pilot
    // "../../outside-the-base" — and nothing may turn that into a path. The
    // slot directory is derived from the numeric id alone, so the name is data
    // and never a directory component. Names a *save* could not hold (empty, or
    // longer than the bound) are refused before an id is allocated, so a
    // refused name allocates nothing.
    let mut session = sandbox(base.path());
    for hostile_name in ["../../outside-the-base", "..", "/etc/passwd", "a/b"] {
        session
            .create(hostile_name)
            .unwrap_or_else(|error| panic!("{hostile_name:?} is a legal display name: {error}"));
    }
    for unusable_name in ["", &"x".repeat(200)] {
        assert!(
            session.create(unusable_name).is_err(),
            "a name a save could not hold is refused: {unusable_name:?}"
        );
    }
    // Every entry in the population directory is a registry file or a slot named
    // by a numeric id: no directory is named after a display name, and the
    // planted malicious save's id was the one the session would have used.
    let names: Vec<String> = fs::read_dir(&population)
        .expect("the population directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    for name in &names {
        assert!(
            name.starts_with("registry.") || name.starts_with("profile-"),
            "no entry is named after a display name: {name}"
        );
    }
    // The refused names allocated nothing: the two unusable ones added no id
    // beyond the four path-shaped ones plus the planted profile.
    assert_eq!(
        session.live().len(),
        5,
        "exactly the accepted names became profiles, and the refused ones none: {:?}",
        session.live()
    );
    // The canary is exactly as it was, and nothing was written above the base.
    assert_eq!(
        fs::read(canary.join("canary")).expect("the canary"),
        b"untouched"
    );
    let entries = fs::read_dir(base.path())
        .expect("the base directory")
        .count();
    assert_eq!(
        entries,
        2,
        "the base holds only the synthetic population and the canary: {}",
        fs::read_dir(base.path())
            .expect("the base directory")
            .map(|entry| entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    );
    drop(session);
}

/// A real process death in the middle of a session's commit: the next session
/// opens the same population, finds a whole revision, reports the fallback and
/// can save again.
#[test]
fn accept_f48_d_a_session_that_was_killed_mid_commit_reopens_whole_and_saves_again() {
    let base = TempBase::new("killed-session");
    let population = base.path().join(ProfileKind::Synthetic.label());
    let id;
    {
        let mut session = sandbox(base.path());
        id = session.create("Crash test").expect("create");
        // Each revision is written through the session's own document, so the
        // money value identifies the revision that is on disk.
        for revision in 2..=3 {
            let document = session.document_mut().expect("the document is mutable");
            document.campaign.money_minor = 1_000 + revision;
            session
                .commit()
                .unwrap_or_else(|error| panic!("revision {revision} is committed: {error}"));
        }
    }
    // Three revisions installed, so `profile.sav` and `profile.bak` are both
    // populated before the crash.
    let ready = base.path().join("ready");
    let _ = fs::remove_file(&ready);
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args([
            "--exact",
            SESSION_CRASH_CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("CS_F48D_APP_CRASH_SLOT", population.join(slot_name(id)))
        .env("CS_F48D_APP_CRASH_ID", id.get().to_string())
        .env("CS_F48D_APP_CRASH_READY", &ready);
    let mut child = command.spawn().expect("the crash child is spawned");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        if let Some(status) = child.try_wait().expect("waited") {
            panic!("the crash child exited early with {status}");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the crash child never reached its crash point");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().expect("killed");
    let status = child.wait().expect("reaped");
    assert!(
        !status.success(),
        "the child died rather than returning: {status}"
    );

    // The next session: the whole revision before or after the crash, reported.
    let mut session = sandbox(base.path());
    assert_eq!(
        session.selected(),
        Some(id),
        "the profile is selected again after a crash"
    );
    let document = session.document().expect("a whole revision").clone();
    assert!(
        document.revision == Revision(3) || document.revision == Revision(4),
        "the whole revision before or after the crash: {:?}",
        document.revision
    );
    // The revision in force is *whole*: every field this session wrote for it is
    // still there. The crash child built revision 4 from the stored document, so
    // revision 4 is revision 3 with one more minor unit — not a mixture of the
    // two files.
    assert_eq!(
        document.campaign.money_minor,
        1_000 + document.revision.0,
        "money identifies the revision that is whole: {:?}",
        document.campaign
    );
    assert_eq!(
        document.display_name, "Crash test",
        "the profile this session created is the one in force"
    );
    if document.revision == Revision(3) {
        assert!(
            session
                .warnings()
                .iter()
                .any(|line| line.contains("profile.tmp") || line.contains("recovered")),
            "the fallback is reported: {:?}",
            session.warnings()
        );
    }
    // And the session carries on: settings and a commit both work.
    session
        .set_setting("audio.volume", "6")
        .expect("a setting change after a crash");
    let revision = session
        .commit()
        .expect("the session saves again after a crash");
    assert_eq!(revision, Revision(document.revision.0 + 1));
    session.finish().expect("the session tears down");

    // A third session sees the committed revision, so the chain of revisions is
    // intact across two crashes-worth of restarts.
    let session = sandbox(base.path());
    assert_eq!(
        session.document().expect("a whole revision").revision,
        Revision(document.revision.0 + 1)
    );
}

/// A session over a population whose slot path is a symbolic link refuses to
/// open that profile and does not read or write through the link, so the
/// population separation rule holds against a planted directory entry too.
#[test]
#[cfg(unix)]
fn accept_f48_d_a_planted_link_at_a_slot_is_refused_by_the_session() {
    let base = TempBase::new("planted-link");
    let population = base.path().join(ProfileKind::Synthetic.label());
    let canary = base.outside();
    let id = {
        let mut session = sandbox(base.path());
        session.create("First").expect("create")
    };

    // The canary is a complete valid save of the same profile, so a read that
    // followed the link would succeed and return it.
    let mut canary_slot = DirStorage::create(&canary, PROFILE_PREFIX).expect("the canary slot");
    commit(&mut canary_slot, &synthetic(id, 77)).expect("the canary save");
    let canary_before = fs::read(canary_slot.path(SaveFile::Current)).expect("the canary bytes");

    let slot = population.join(slot_name(id));
    let aside = population.join("moved-aside");
    fs::rename(&slot, &aside).expect("the slot is moved aside");
    std::os::unix::fs::symlink(&canary, &slot).expect("a planted link");

    let mut session = sandbox(base.path());
    let selected = session.selected();
    // The registry still names the pilot active, but the slot cannot be read.
    if selected == Some(id) {
        // If it did select, the document must not be the canary's.
        let document = session.document().expect("a document");
        assert_ne!(
            document.campaign.money_minor, 1_077,
            "the canary's revision was never adopted"
        );
    }
    assert!(
        session
            .select(id)
            .err()
            .is_some_and(|error| error.to_string().contains("not a directory")),
        "selecting through the link is refused: {:?}",
        session.select(id).map_err(|error| error.to_string())
    );
    assert_eq!(
        fs::read(canary_slot.path(SaveFile::Current)).expect("the canary bytes"),
        canary_before,
        "nothing outside the population was read into it or written through it"
    );
}

/// The crash child for the session test: it performs the real commit for a real
/// revision and stops before the install, so the parent kills it there.
#[test]
#[ignore = "the crash child, spawned and killed by the test above"]
fn f48_d_session_crash_child_commits_and_dies_before_the_install() {
    let Ok(slot) = std::env::var("CS_F48D_APP_CRASH_SLOT") else {
        println!("no crash request in the environment; nothing to do");
        return;
    };
    let id = ProfileId::new(
        std::env::var("CS_F48D_APP_CRASH_ID")
            .expect("the id in the request")
            .parse()
            .expect("an id"),
    )
    .expect("nonzero");
    let ready = std::env::var("CS_F48D_APP_CRASH_READY").expect("the ready marker path");

    // The production phases, performed for real, with the stop placed between
    // them. This mirrors the phase sequence rather than replacing it: the file
    // operations are the production ones, and the point of the kill is between
    // them.
    //
    // The revision offered is built from what is *stored*, exactly as a real
    // save is: recovery prefers the newest whole revision, and a child that
    // offered a document of its own invention would be measuring its fixture
    // rather than the write path.
    let directory = PathBuf::from(slot);
    let stored = load_profile_slot(&directory)
        .expect("the slot is readable")
        .expect("the slot holds a revision");
    assert_eq!(stored.profile_id, id, "the slot is this profile's");
    let mut document = stored;
    document.revision = document.revision.next().expect("a successor");
    document.campaign.money_minor += 1;
    let bytes = cs_content::save::codec::encode(&document).expect("the revision encodes");
    let mut storage = DirStorage::new(&directory, PROFILE_PREFIX);
    storage.write_temp(&bytes).expect("the temp write");
    storage.sync_temp().expect("the temp sync");
    if fs::read_dir(&directory)
        .expect("the slot directory")
        .any(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .starts_with("profile.sav")
        })
    {
        storage.rotate_backup().expect("the backup rotation");
    }
    fs::write(&ready, b"ready").expect("the ready marker");
    // Blocked here so the parent kills this process between the rotation and the
    // install — the window where `profile.sav` does not exist and the newest
    // whole state is only in `profile.tmp`.
    loop {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The child names the phase it stops at, so the intent is checkable from the
/// child's own name rather than only from this file's comment.
const SESSION_CRASH_CHILD: &str = "f48_d_session_crash_child_commits_and_dies_before_the_install";

/// A `ProfileSession` open over a population whose registry belongs to another
/// kind is refused, so a synthetic session cannot reach a production pilot even
/// when the registry file is the only thing wrong.
#[test]
fn accept_f48_d_a_session_never_reaches_another_population_through_the_registry() {
    let base = TempBase::new("wrong-kind");
    // A production population with a real profile in it. Automation may not open
    // it at all, which is the outer rule; the inner case is a production-shaped
    // registry dropped into the synthetic subtree.
    let production = base.path().join(ProfileKind::Production.label());
    {
        let mut interactive = ProfileSession::open(
            base.path(),
            SessionOrigin::Interactive,
            ProfileKind::Production,
            &catalog(),
        )
        .expect("an interactive production session opens");
        interactive.create("Real pilot").expect("create");
        interactive.finish().expect("teardown");
    }
    let synthetic = base.path().join(ProfileKind::Synthetic.label());
    fs::create_dir_all(&synthetic).expect("the synthetic population");
    fs::copy(
        production.join(SaveFile::Current.prefixed_name("registry")),
        synthetic.join(SaveFile::Current.prefixed_name("registry")),
    )
    .expect("the production registry is planted in the synthetic subtree");

    // The registry slot belongs to another population, so the synthetic session
    // refuses to open at all rather than opening a population it does not own.
    let error = ProfileSession::open_sandbox(base.path(), &catalog())
        .expect_err("a foreign registry is refused");
    let text = error.to_string();
    assert!(
        text.contains("production") && text.contains("synthetic"),
        "the refusal names both populations: {text}"
    );
    // The planted bytes are untouched: this build never adopted them and never
    // wrote over them.
    assert_eq!(
        fs::read(synthetic.join(SaveFile::Current.prefixed_name("registry")))
            .expect("the planted registry"),
        fs::read(production.join(SaveFile::Current.prefixed_name("registry")))
            .expect("the production registry"),
        "the foreign registry's bytes were not overwritten"
    );
    // The production population is untouched by the synthetic session.
    let reopened = ProfileLibrary::open(production, ProfileKind::Production)
        .expect("the production population still opens");
    assert_eq!(
        reopened.live().len(),
        1,
        "the production pilot is still there"
    );
    assert_eq!(
        reopened.active().map(|id| id.get()),
        Some(1),
        "and still selected"
    );
    assert!(
        reopened.status().warning_lines().is_empty(),
        "with nothing to reconcile: {:?}",
        reopened.status().warning_lines()
    );
}

/// A save whose campaign run names a valid key is carried through a crash, so
/// the campaign state a player resumes on is the one that was committed.
#[test]
fn accept_f48_d_campaign_state_survives_a_real_process_death() {
    let base = TempBase::new("campaign-survives");
    let population = base.path().join(ProfileKind::Synthetic.label());
    let id;
    {
        let mut session = sandbox(base.path());
        id = session.create("Campaign").expect("create");
        session
            .begin_campaign_run("run-a")
            .expect("a campaign run begins");
        assert_eq!(
            session
                .record_outcome("m01.complete")
                .expect("the outcome is recorded"),
            cs_app::profile::OutcomeRecord::Recorded,
            "the first outcome is recorded"
        );
        assert_eq!(
            session
                .record_outcome("m01.complete")
                .expect("the replay is suppressed"),
            cs_app::profile::OutcomeRecord::AlreadyApplied,
            "and the replay is suppressed"
        );
    }
    let ready = base.path().join("ready-campaign");
    let _ = fs::remove_file(&ready);
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args([
            "--exact",
            SESSION_CRASH_CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("CS_F48D_APP_CRASH_SLOT", population.join(slot_name(id)))
        .env("CS_F48D_APP_CRASH_ID", id.get().to_string())
        .env("CS_F48D_APP_CRASH_READY", &ready);
    let mut child = command.spawn().expect("spawned");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        if let Some(status) = child.try_wait().expect("waited") {
            panic!("the crash child exited early with {status}");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the crash child never reached its crash point");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().expect("killed");
    assert!(!child.wait().expect("reaped").success(), "the child died");

    let session = sandbox(base.path());
    let document = session.document().expect("a whole revision");
    assert_eq!(
        document.campaign.run_id.as_deref(),
        Some("run-a"),
        "the campaign run that was committed is the one in force"
    );
    assert!(
        document
            .campaign
            .applied_outcomes
            .contains(&"m01.complete".to_owned()),
        "and the applied outcome survived the crash, so it cannot be paid twice: {:?}",
        document.campaign.applied_outcomes
    );
    // The replay is still suppressed after the crash, which is the whole point
    // of the applied list being written atomically with the profile.
    let mut session = session;
    assert_eq!(
        session
            .record_outcome("m01.complete")
            .expect("the replay after a crash is suppressed"),
        cs_app::profile::OutcomeRecord::AlreadyApplied,
        "the replay after a crash is still suppressed"
    );
}
