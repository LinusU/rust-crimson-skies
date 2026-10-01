//! Acceptance scenarios F48-B: the application-side profile path — the
//! population rule at the point a path is produced, and the library a runtime
//! session opens. Task test prefix: `accept_f48_b_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`
//! (non-negotiable 3 and 4); contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Persistence"). Every profile written here is newly authored synthetic
//! data in a temporary directory: no test opens a real user's profile tree,
//! `$CS_GAME_DIR` or any original data, and each tree is removed on drop.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{
    LibraryOpenError, ProfileDirError, SessionOrigin, open_library, population_dir, slot_dir,
};
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision};

/// A disposable user-data base, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-b-app-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pid(n: u64) -> ProfileId {
    ProfileId::new(n).expect("nonzero")
}

fn document(kind: ProfileKind, id: ProfileId) -> ProfileDocument {
    let mut d = ProfileDocument::synthetic(id, Revision(1));
    d.kind = kind;
    d
}

/// Automation never gets the live production tree, and the refusal is the same
/// one the path helper has always given — the library cannot be opened around
/// it, because the directory is produced by the rule.
#[test]
fn accept_f48_b_automation_never_opens_the_production_library() {
    let base = TempBase::new("automation");
    assert_eq!(
        open_library(
            base.path(),
            SessionOrigin::Automated,
            ProfileKind::Production
        )
        .map(|_| ()),
        Err(LibraryOpenError::Dir(
            ProfileDirError::AutomatedProductionAccess
        ))
    );
    assert_eq!(
        population_dir(
            base.path(),
            SessionOrigin::Automated,
            ProfileKind::Production
        ),
        Err(ProfileDirError::AutomatedProductionAccess),
        "the population root is refused on the same rule as a slot"
    );
    assert!(
        !base.path().join("production").exists(),
        "the refusal happens before anything is created"
    );

    // A person playing may, and the other populations may too.
    open_library(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
    )
    .expect("a person may open the production tree");
    for kind in [
        ProfileKind::Synthetic,
        ProfileKind::Modded,
        ProfileKind::Evidence,
    ] {
        open_library(base.path(), SessionOrigin::Automated, kind)
            .unwrap_or_else(|e| panic!("automation may use {}: {e}", kind.label()));
    }
    // And the populations are separate directories, so a profile written into
    // one is not visible in another.
    let mut production = open_library(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
    )
    .expect("open production");
    let (live, _) = production
        .create(document(ProfileKind::Production, pid(1)))
        .expect("create a production profile");
    let evidence = open_library(base.path(), SessionOrigin::Automated, ProfileKind::Evidence)
        .expect("open evidence");
    assert!(
        evidence.live().is_empty(),
        "the evidence population sees no production profile"
    );
    assert!(
        !evidence.load(live).expect("load").is_present(),
        "and cannot read its slot"
    );
    assert_eq!(
        slot_dir(
            base.path(),
            SessionOrigin::Interactive,
            ProfileKind::Production,
            live
        )
        .expect("slot"),
        production.slot_dir(live),
        "the library and the path helper agree on where a slot lives"
    );
}

/// The library a session opens hands out persistent ids and a surviving
/// registry, and its recovery text is available to a caller that shows it.
#[test]
fn accept_f48_b_the_opened_library_persists_ids_and_reports_recovery() {
    let base = TempBase::new("library");
    let (first, second) = {
        let mut library = open_library(
            base.path(),
            SessionOrigin::Interactive,
            ProfileKind::Production,
        )
        .expect("open");
        assert!(library.status().registry_warnings.is_empty());
        assert!(!library.status().registry_persisted, "a new population");
        let (a, mut first_document) = library
            .create(document(ProfileKind::Production, pid(1)))
            .expect("create a");
        let (b, _) = library
            .create(document(ProfileKind::Production, pid(1)))
            .expect("create b");
        first_document.revision = Revision(2);
        library
            .save(&first_document)
            .expect("save a second revision");
        library.delete(b).expect("delete b");
        (a, b)
    };

    // Reopening finds the same state: the first profile's second revision, no
    // trace of the deleted one, and a mark above both ids ever issued.
    let library = open_library(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
    )
    .expect("reopen");
    assert!(
        library.status().registry_persisted,
        "the registry is a file"
    );
    assert_eq!(library.live(), &[first]);
    assert!(library.status().high_water >= second.get());
    let loaded = library.load(first).expect("load");
    assert_eq!(
        loaded.document.as_ref().expect("a revision").revision,
        Revision(2),
        "the newest whole revision survived the restart"
    );
    assert!(loaded.warnings.is_empty(), "nothing to report");
    assert!(!library.load(second).expect("load").is_present());

    // Damage the newest file and the library says so in text.
    let directory = library.slot_dir(first);
    let mut damaged = fs::read(directory.join("profile.sav")).expect("read");
    damaged[30] ^= 0x11;
    fs::write(directory.join("profile.sav"), &damaged).expect("write");
    let loaded = library.load(first).expect("load");
    assert_eq!(
        loaded.document.as_ref().expect("a revision").revision,
        Revision(1),
        "the backup is in force"
    );
    assert!(
        !loaded.warnings.is_empty(),
        "a fallback is never silent: {:?}",
        loaded.warnings
    );
}
