//! Acceptance scenarios F48-C: the runtime profile session — settings, campaign
//! state and sandbox profile ownership, wired into the producer and consumer
//! F48-B built. Task test prefix: `accept_f48_c_`.
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`
//! (non-negotiable 3, 4 and 5; acceptance cases AC02, AC03, AC04); contract
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Session reset", "Outcome and
//! economy transaction", "Persistence").
//!
//! Every profile written here is newly authored synthetic data in a temporary
//! directory: no test opens a real user's profile tree, `$CS_GAME_DIR` or any
//! original data, and every tree is removed on drop. The settings *keys* below
//! come from a catalog this test declares — they are engine vocabulary invented
//! here to exercise the machinery, not a claim about any original setting.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{
    ClaimError, MAX_COMMIT_ATTEMPTS, OutcomeRecord, ProfileSession, SessionError, SessionOrigin,
};
use cs_content::save::library::{LibraryError, ProfileLibrary};
use cs_content::save::settings::{
    RefusalReason, SettingCatalog, SettingOutcome, SettingRule, ValueRule,
};
use cs_types::profile::{ProfileId, ProfileKind, SettingApply, SettingEntry};

/// A disposable user-data base, removed on drop.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f48-c-{label}-{}-{}",
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
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The catalog this test declares: one live setting, one that needs a restart
/// and one bounded integer. Nothing here is an original-game setting.
fn catalog() -> SettingCatalog {
    SettingCatalog::new([
        SettingRule {
            key: "video.detail",
            apply: SettingApply::Live,
            value: ValueRule::Choice(&["low", "high"]),
            default: "high",
        },
        SettingRule {
            key: "video.driver",
            apply: SettingApply::RestartRequired,
            value: ValueRule::Choice(&["d3d9", "opengl"]),
            default: "d3d9",
        },
        SettingRule {
            key: "sound.device",
            apply: SettingApply::Live,
            value: ValueRule::Integer { min: 0, max: 3 },
            default: "0",
        },
    ])
    .expect("the fixture catalog holds together")
}

fn production(base: &TempBase) -> Result<ProfileSession, SessionError> {
    ProfileSession::open(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
        &catalog(),
    )
}

/// The live production profile tree of a base, as the population rule produces
/// it for a person playing.
fn production_directory(base: &TempBase) -> PathBuf {
    cs_app::profile::population_dir(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
    )
    .expect("a person may open the production tree")
}

/// The minimum acceptance scenario (AC03): create, delete, create — and prove
/// that an id from before the deletion never comes to mean a different pilot,
/// inside this session or after a restart of it.
#[test]
fn accept_f48_c_an_id_from_a_deleted_profile_never_names_a_new_pilot() {
    let base = TempBase::new("ids");
    let (first, second) = {
        let mut session = production(&base).expect("open a production session");

        let first = session.create("First Pilot").expect("create");
        assert_eq!(session.live(), &[first]);
        assert_eq!(session.selected(), Some(first));
        let document = session.document().expect("a document");
        assert_eq!(
            document.profile_id, first,
            "the document written carries the id the library allocated: the id \
             is the identity, not a label derived afterwards"
        );
        assert_eq!(document.display_name, "First Pilot");

        let second = session.create("Second Pilot").expect("create");
        assert!(
            second > first,
            "a new profile is issued above the high-water mark"
        );

        // Give the first profile some progression, so a stale copy of it would be
        // distinguishable from what is stored.
        session.select(first).expect("select the first profile");
        session
            .commit_with(|document| {
                document.campaign.money_minor = 500;
                Ok(())
            })
            .expect("write the first profile's campaign state");

        session.delete(first).expect("delete the first profile");
        assert_eq!(session.live(), &[second], "the deletion took effect");
        assert_ne!(
            session.selected(),
            Some(first),
            "a session must not keep playing a profile it just deleted"
        );
        assert_eq!(
            session.document().map(|document| document.profile_id),
            session.selected(),
            "whatever is selected now carries its own id in its document"
        );
        session.finish().expect("teardown");
        (first, second)
    };

    // Across a restart the deleted id is still not live, the survivor is still
    // the one the persisted pointer names, and a new profile is issued above
    // every id this population has ever issued.
    let third = {
        let mut session = production(&base).expect("reopen");
        assert_eq!(
            session.live(),
            &[second],
            "the deleted profile is not live after the restart"
        );
        assert_eq!(
            session.selected(),
            Some(second),
            "the active pointer names the id, not a list position"
        );
        assert_eq!(
            session
                .document()
                .map(|document| document.display_name.as_str()),
            Some("Second Pilot"),
            "and the survivor is the pilot the id was issued to"
        );
        assert_eq!(session.document().expect("a document").profile_id, second);
        let third = session.create("Third Pilot").expect("create");
        assert!(
            third > second && third > first,
            "the new id is above both ids ever issued"
        );
        assert_eq!(session.live(), &[second, third]);
        session.finish().expect("teardown");
        third
    };
    assert!(third > second && third > first);

    // The retired slot is still on disk under the deleted id and is not a
    // profile: the highest live id is below the issued ones, and a session that
    // walks the whole population never offers the deleted id as a pilot.
    let session = production(&base).expect("open again");
    assert_eq!(session.live(), &[second, third]);
    assert!(
        !session.live().contains(&first),
        "an issued-and-deleted id is never live again"
    );
    assert!(
        !session.library().slot_dir(first).exists(),
        "the deleted profile's slot is no longer a live slot"
    );
    let retired = session
        .library()
        .base()
        .join(cs_content::save::library::retired_name(first));
    assert!(
        retired.exists(),
        "the deleted profile's files are kept under its own id, retired, not destroyed"
    );
    assert_eq!(
        cs_content::save::library::load_profile_slot(&retired)
            .expect("a retired slot is readable")
            .map(|document| document.profile_id),
        Some(first),
        "the kept files are the deleted pilot's, never a new one"
    );
    session.finish().expect("teardown");
}

/// A population has one live owner at a time, and the claim is released both by
/// an explicit teardown and by an abandoned session, so an error path cannot
/// leave a profile tree permanently unopenable.
#[test]
fn accept_f48_c_a_population_has_one_live_owner_and_teardown_releases_it() {
    let base = TempBase::new("claim");
    let session = production(&base).expect("open");
    let directory = session.directory().to_path_buf();

    let second_open = production(&base);
    assert!(
        matches!(
            second_open.as_ref().err(),
            Some(SessionError::Claim(ClaimError::AlreadyHeld { .. }))
        ),
        "a second live session must be refused, got {:?}",
        second_open.as_ref().err()
    );

    // A different population in the same base is a different resource: the claim
    // is per population directory, not per base.
    ProfileSession::open(
        base.path(),
        SessionOrigin::Interactive,
        ProfileKind::Modded,
        &catalog(),
    )
    .expect("another population opens independently")
    .finish()
    .expect("teardown");

    let report = session.finish().expect("teardown");
    assert_eq!(report.population, directory);
    production(&base)
        .expect("the population is openable again after teardown")
        .finish()
        .expect("teardown");

    // An abandoned session — dropped without `finish` — also releases it, so a
    // `?` on an error path cannot strand ownership.
    {
        let _abandoned = production(&base).expect("open");
        // dropped here, without `finish`
    }
    production(&base)
        .expect("an abandoned session does not strand the claim")
        .finish()
        .expect("teardown");
}

/// Settings are validated against the declared catalog, and a change that needs
/// a restart is labeled and deferred rather than applied under the running
/// session (spec non-negotiable 5).
#[test]
fn accept_f48_c_a_restart_labeled_setting_is_stored_but_not_applied_now() {
    let base = TempBase::new("settings");
    {
        let mut session = production(&base).expect("open");
        session.create("Pilot").expect("create");

        // A live setting takes effect at once.
        assert_eq!(
            session
                .set_setting("video.detail", "low")
                .expect("set a live setting"),
            SettingOutcome::AppliedLive {
                key: "video.detail".to_owned(),
                previous: "high".to_owned(),
                value: "low".to_owned(),
            },
            "a live setting is applied immediately"
        );
        assert_eq!(
            session
                .settings()
                .expect("settings")
                .live_value("video.detail"),
            Some("low")
        );

        // A restart-required setting is stored and labeled, and the running
        // session keeps the value it started with.
        let outcome = session
            .set_setting("video.driver", "opengl")
            .expect("set a restart setting");
        assert_eq!(
            outcome,
            SettingOutcome::AppliedAfterRestart {
                key: "video.driver".to_owned(),
                previous: "d3d9".to_owned(),
                value: "opengl".to_owned(),
            },
            "the change is labeled as needing a restart"
        );
        assert!(outcome.needs_restart());
        let settings = session.settings().expect("settings");
        assert_eq!(
            settings.live_value("video.driver"),
            Some("d3d9"),
            "the running session keeps the value it started with"
        );
        assert_eq!(
            settings.stored_value("video.driver"),
            Some("opengl"),
            "the new value is what the save will hold"
        );
        assert!(settings.needs_restart());
        assert_eq!(settings.pending_restart(), ["video.driver".to_owned()]);

        assert!(
            session.has_uncommitted_settings(),
            "a settings change is not on disk until it is committed"
        );
        assert_eq!(session.commit().expect("commit the settings").0, 2);
        assert!(!session.has_uncommitted_settings());
        session.finish().expect("teardown");
    }

    // A new session is the restart: the deferred change is now in force and the
    // live setting was not lost.
    let session = production(&base).expect("reopen");
    let settings = session.settings().expect("settings");
    assert_eq!(settings.live_value("video.driver"), Some("opengl"));
    assert_eq!(settings.live_value("video.detail"), Some("low"));
    assert!(
        !settings.needs_restart(),
        "a fresh session has nothing pending"
    );
    session.finish().expect("teardown");
}

/// A setting value a rule refuses changes nothing: the value that was in force
/// stays in force and the refusal is reported as text. That is the safe recovery
/// path the spec requires for a device or display setting that cannot be used.
#[test]
fn accept_f48_c_a_refused_setting_value_recovers_to_the_value_in_force() {
    let base = TempBase::new("refused");
    let mut session = production(&base).expect("open");
    session.create("Pilot").expect("create");
    session
        .set_setting("sound.device", "2")
        .expect("set a valid device");
    session.commit().expect("commit the accepted device");
    assert!(!session.has_uncommitted_settings());

    // Out of the declared range: refused, and the device stays.
    assert_eq!(
        session
            .set_setting("sound.device", "9")
            .expect("an unusable value is a refusal, not a failure"),
        SettingOutcome::Refused {
            key: "sound.device".to_owned(),
            offered: "9".to_owned(),
            reason: RefusalReason::OutOfRange,
            recovery: "2".to_owned(),
        }
    );
    assert_eq!(
        session
            .settings()
            .expect("settings")
            .live_value("sound.device"),
        Some("2"),
        "the last acceptable value stays in force"
    );
    assert!(
        !session.has_uncommitted_settings(),
        "a refused value stores nothing, so nothing is owed to the disk"
    );

    // Not one of the declared labels at all.
    assert!(matches!(
        session
            .set_setting("video.detail", "ultra")
            .expect("refused"),
        SettingOutcome::Refused {
            reason: RefusalReason::BadValue,
            recovery,
            ..
        } if recovery == "high"
    ));

    // A key this build declares no rule for is never interpreted.
    assert!(matches!(
        session.set_setting("video.bloom", "on").expect("refused"),
        SettingOutcome::Refused {
            reason: RefusalReason::UnknownKey,
            ..
        }
    ));

    // Every refusal is text a caller can show.
    let warnings = session.warnings();
    assert!(
        warnings.iter().any(|line| line.contains("sound.device=9")),
        "the refusal names the value: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("outside the declared range")
                && line.contains("\"2\" is in force")),
        "and names the reason and what is in force instead: {warnings:?}"
    );

    // Only the accepted value was written: the refused one never reached disk.
    session.finish().expect("teardown");
    let session = production(&base).expect("reopen");
    let settings = session.settings().expect("settings");
    assert_eq!(
        settings.stored_value("sound.device"),
        Some("2"),
        "a refused value never reached the save"
    );
    assert_eq!(
        settings.stored_value("video.detail"),
        Some("high"),
        "and the refused label changed nothing either"
    );
    assert!(
        !settings.stored_value("video.bloom").is_some(),
        "an undeclared key is not stored by this build"
    );
    session.finish().expect("teardown");
}

/// Campaign state belongs to the profile and is written as one whole revision
/// with the rest of the document. An outcome id cannot be paid twice: a replay
/// after a crash before acknowledgment is a no-op that writes nothing.
#[test]
fn accept_f48_c_campaign_state_is_the_profiles_and_an_outcome_replays_once() {
    let base = TempBase::new("campaign");
    let (first_profile, written_revision) = {
        let mut session = production(&base).expect("open");
        let profile = session.create("Campaign Pilot").expect("create");

        let started = session
            .begin_campaign_run("campaign.one")
            .expect("begin a run");
        assert_eq!(started.0, 2, "a run start is the revision after creation");
        assert_eq!(
            session.campaign().expect("campaign").run_id.as_deref(),
            Some("campaign.one")
        );

        // The first application of an outcome id writes it.
        assert_eq!(
            session.record_outcome("m01.complete").expect("record"),
            OutcomeRecord::Recorded
        );
        let stored = session.document().expect("a document").clone();
        assert_eq!(
            stored.campaign.applied_outcomes,
            ["m01.complete".to_owned()]
        );

        // The replay writes nothing: not a second entry, not a new revision.
        // This is what makes a crash before the acknowledgment safe to retry.
        assert_eq!(
            session.record_outcome("m01.complete").expect("replay"),
            OutcomeRecord::AlreadyApplied
        );
        let after_replay = session.document().expect("a document");
        assert_eq!(
            after_replay.campaign.applied_outcomes,
            ["m01.complete".to_owned()],
            "the replay added nothing"
        );
        assert_eq!(
            after_replay.revision, stored.revision,
            "the replay wrote no revision"
        );

        // An outcome that is not a usable key is refused before anything is
        // written, and the profile's state is untouched.
        assert!(matches!(
            session.record_outcome("m01 complete").expect_err("refused"),
            SessionError::Refused(_)
        ));
        assert_eq!(
            session
                .document()
                .expect("a document")
                .campaign
                .applied_outcomes,
            ["m01.complete".to_owned()]
        );

        session.finish().expect("teardown");
        (profile, stored.revision)
    };

    // The campaign state survived the session.
    let mut session = production(&base).expect("reopen");
    let document = session.document().expect("a document").clone();
    assert_eq!(document.campaign.run_id.as_deref(), Some("campaign.one"));
    assert_eq!(
        document.campaign.applied_outcomes,
        ["m01.complete".to_owned()]
    );
    assert_eq!(document.revision, written_revision);

    // And it belongs to that profile only: a new pilot starts with no run and no
    // applied outcomes.
    let other = session.create("Other Pilot").expect("create");
    assert_ne!(other, first_profile);
    let fresh = session.document().expect("a document");
    assert_eq!(fresh.profile_id, other);
    assert_eq!(fresh.revision.0, 1, "a new profile starts at revision 1");
    assert!(
        fresh.campaign.run_id.is_none() && fresh.campaign.applied_outcomes.is_empty(),
        "a new profile does not inherit another pilot's campaign"
    );
    session.finish().expect("teardown");
}

/// A commit that finds the stored revision moved refreshes the view and
/// re-applies the change to what is actually stored, so a concurrent writer's
/// unrelated work survives (contract: a conflicting revision "fail and refresh
/// the view; they do not overwrite unrelated progression").
#[test]
fn accept_f48_c_a_conflicting_commit_reapplies_to_the_stored_revision() {
    let base = TempBase::new("conflict");
    let mut session = production(&base).expect("open");
    let id = session.create("Pilot").expect("create");
    let directory = session.directory().to_path_buf();

    // Another writer moves the profile underneath this session. A bare library
    // is used deliberately: it is the same production path a second process
    // would take, and the revision check is what exists to catch it.
    move_the_profile(&directory, id, 1234);

    // This session's copy is now stale. It refreshes and re-applies, and the
    // other writer's money survives.
    let applied = session
        .commit_with(|document| {
            document
                .campaign
                .applied_outcomes
                .push("m01.complete".to_owned());
            Ok(())
        })
        .expect("the change is re-applied to the stored revision");
    let document = session.document().expect("a document");
    assert_eq!(
        document.campaign.money_minor, 1234,
        "the concurrent writer's work is not overwritten"
    );
    assert_eq!(
        document.campaign.applied_outcomes,
        ["m01.complete".to_owned()],
        "and this session's change landed on top of it"
    );
    assert_eq!(document.revision, applied);

    // The plain commit holds no record of what it meant to change, so it does
    // not retry: the library's conflict is reported instead of a silent discard.
    move_the_profile(&directory, id, 4321);
    session
        .document_mut()
        .expect("a document")
        .campaign
        .applied_outcomes
        .push("m02.complete".to_owned());
    assert!(
        matches!(session.commit(), Err(SessionError::Library(_))),
        "a stale plain commit reports the library's conflict"
    );
    session.finish().expect("teardown");

    // The other writer's work is still there: the refused commit wrote nothing.
    let mut session = production(&base).expect("reopen");
    let document = session.document().expect("a document").clone();
    assert_eq!(
        document.campaign.money_minor, 4321,
        "a refused commit overwrote nothing"
    );
    assert!(
        !document
            .campaign
            .applied_outcomes
            .contains(&"m02.complete".to_owned()),
        "and the refused change is not half-applied"
    );
    // A fresh session commits cleanly.
    session
        .commit_with(|document| {
            document
                .campaign
                .applied_outcomes
                .push("m03.complete".to_owned());
            Ok(())
        })
        .expect("a fresh session commits cleanly");
    session.finish().expect("teardown");
}

/// Commits give up rather than retrying forever, and report the conflict with
/// the stored revision the caller has to refresh from.
#[test]
fn accept_f48_c_a_continuously_moved_profile_is_reported_not_retried_forever() {
    let base = TempBase::new("starved");
    let mut session = production(&base).expect("open");
    let id = session.create("Pilot").expect("create");
    let directory = session.directory().to_path_buf();

    // Every attempt is preceded by another writer taking the next revision, so
    // this session never wins the race.
    let mut moves = 0;
    let outcome = session.commit_with(|document| {
        moves += 1;
        move_the_profile(&directory, id, moves);
        document.campaign.money_minor = 100 + moves;
        Ok(())
    });
    let error = outcome.expect_err("a continuously moved profile is reported");
    match &error {
        SessionError::Conflict {
            stored, attempts, ..
        } => {
            assert_eq!(*attempts, MAX_COMMIT_ATTEMPTS);
            assert!(stored.0 > 1, "the stored revision is reported");
        }
        other => panic!("expected a conflict, got {other}"),
    }
    assert!(
        error.to_string().contains("moved to revision"),
        "the conflict names the revision the caller must refresh from: {error}"
    );
    session.finish().expect("teardown");
}

/// A future or malformed document is refused without a panic and without a
/// destructive overwrite: the session reports the failure and the bytes on disk
/// are exactly what was there before.
#[test]
fn accept_f48_c_a_hostile_save_is_refused_without_overwriting_it() {
    let base = TempBase::new("hostile");
    let (slot, original) = {
        let mut session = production(&base).expect("open");
        let id = session.create("Pilot").expect("create");
        session
            .commit_with(|document| {
                document.campaign.money_minor = 7;
                Ok(())
            })
            .expect("commit so there is a backup to recover");
        let slot = session.library().slot_dir(id);
        let original = fs::read(slot.join("profile.sav")).expect("read the stored save");
        session.finish().expect("teardown");
        (slot, original)
    };

    // A future schema: an unreadable major is refused outright, never shadowed
    // by an older file and never overwritten.
    let mut future = original.clone();
    let header_end = future
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("the save has a header line");
    future.splice(0..header_end, b"CSSAVE 9.0".iter().copied());
    fs::write(slot.join("profile.sav"), &future).expect("write a future save");
    // The population opens — one unreadable save does not hide the others — but the
    // profile it names cannot be selected, and the caller is told why.
    let session = production(&base).expect("open");
    assert_eq!(
        session.selected(),
        None,
        "a profile whose save cannot be read is not selected"
    );
    assert_eq!(session.live(), &[ProfileId::new(1).expect("nonzero")]);
    let warnings = session.warnings();
    assert!(
        warnings.iter().any(|line| line.contains("9.0")),
        "an unreadable schema is reported on open, not silently ignored: \
         {warnings:?}"
    );
    assert_eq!(
        fs::read(slot.join("profile.sav")).expect("read"),
        future,
        "the refused bytes are still exactly what was there"
    );
    session.finish().expect("teardown");

    // A body with no valid revision decodes to nothing; recovery falls back to
    // the backup and says so, which is AC02 through the runtime path.
    fs::write(
        slot.join("profile.sav"),
        b"CSSAVE 1.0\nprofile_id=1\nkind=production\n",
    )
    .expect("truncate");
    let session = production(&base).expect("open");
    let document = session.document().expect("a document");
    assert_eq!(
        document.campaign.money_minor, 0,
        "the backup revision is in force"
    );
    let warnings = session.warnings();
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("profile.sav") && line.contains("did not decode")),
        "the damaged file is reported: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("recovered from") && line.contains("profile.bak")),
        "and the fallback is visible, never silent: {warnings:?}"
    );
    assert_ne!(
        fs::read(slot.join("profile.sav")).expect("read"),
        original,
        "the damaged file was kept as it was, not overwritten"
    );
    session.finish().expect("teardown");
}

/// A sandbox session is an automated session over the synthetic population. It
/// cannot reach a production profile at all, and it numbers its own ids from
/// its own high-water mark in its own subtree (spec non-negotiable 4).
#[test]
fn accept_f48_c_a_sandbox_session_never_reaches_a_production_profile() {
    let base = TempBase::new("sandbox");

    // A person opens the production population and creates a pilot.
    let production_id = {
        let mut session = production(&base).expect("open production");
        let id = session.create("Real Pilot").expect("create");
        session.finish().expect("teardown");
        id
    };

    // Automation is refused the production population outright, before anything
    // is created.
    assert!(
        ProfileSession::open(
            base.path(),
            SessionOrigin::Automated,
            ProfileKind::Production,
            &catalog(),
        )
        .is_err(),
        "an automated session may not open the production population"
    );

    let mut sandbox =
        ProfileSession::open_sandbox(base.path(), &catalog()).expect("open the sandbox");
    assert_eq!(sandbox.kind(), ProfileKind::Synthetic);
    assert!(sandbox.live().is_empty());
    assert_eq!(
        sandbox
            .directory()
            .file_name()
            .and_then(|name| name.to_str()),
        Some("synthetic"),
        "the sandbox has its own subtree"
    );
    let sandbox_id = sandbox.create("Sandbox Pilot").expect("create");
    assert!(
        sandbox.live().contains(&sandbox_id),
        "the sandbox issues its own live id"
    );
    assert_eq!(
        sandbox.document().expect("a document").kind,
        ProfileKind::Synthetic,
        "a sandbox profile is synthetic, not a production pilot"
    );
    assert_eq!(
        sandbox.document().expect("a document").display_name,
        "Sandbox Pilot",
        "the sandbox has its own pilot, not the player's"
    );
    assert!(
        sandbox.directory() != production_directory(&base),
        "the sandbox is a different directory from the live profile tree"
    );
    sandbox.finish().expect("teardown");

    // The production pilot is untouched, and production still cannot reach the
    // sandbox population. The two populations number their ids independently,
    // so a numeric collision between them is possible and is not a shared
    // identity — what must not happen is one population's session offering the
    // other population's state.
    let session = production(&base).expect("reopen production");
    assert_eq!(session.live(), &[production_id]);
    assert_eq!(
        session.document().expect("a document").display_name,
        "Real Pilot",
        "the sandbox session changed nothing the player owns"
    );
    let sandbox_directory = cs_app::profile::population_dir(
        base.path(),
        SessionOrigin::Automated,
        ProfileKind::Synthetic,
    )
    .expect("the sandbox root");
    assert_ne!(
        session.library().base(),
        sandbox_directory.as_path(),
        "a production session's population is not the sandbox population"
    );
    assert!(
        session
            .library()
            .base()
            .starts_with(production_directory(&base)),
        "and it is the live production tree"
    );
    assert!(
        sandbox_directory.starts_with(base.path())
            && !sandbox_directory.starts_with(production_directory(&base)),
        "the sandbox subtree is a sibling of the production tree"
    );
    session.finish().expect("teardown");
}

/// A document belonging to another population is refused by the library, so a
/// sandbox profile's document can never be installed into the production tree
/// even by a caller that built one itself.
#[test]
fn accept_f48_c_a_document_from_another_population_is_refused() {
    let base = TempBase::new("foreign");
    let mut session = production(&base).expect("open production");
    let id = session.create("Real Pilot").expect("create");

    let mut foreign = session.document().expect("a document").clone();
    foreign.kind = ProfileKind::Synthetic;
    assert!(
        matches!(
            session.library_mut().save(&foreign),
            Err(LibraryError::ForeignDocument { .. })
        ),
        "a foreign document is refused, not installed"
    );

    let document = session.document().expect("a document");
    assert_eq!(document.kind, ProfileKind::Production);
    assert_eq!(document.profile_id, id);
    assert_eq!(document.revision.0, 1, "the refused write advanced nothing");
    session.finish().expect("teardown");
}

/// Settings belong to the profile, not the session: two profiles in one
/// population keep separate settings and switching profiles switches them.
#[test]
fn accept_f48_c_settings_belong_to_the_profile_not_the_session() {
    let base = TempBase::new("per-profile");
    let mut session = production(&base).expect("open");
    let first = session.create("First").expect("create");
    session
        .set_setting("video.detail", "low")
        .expect("set on the first profile");
    session.commit().expect("commit");

    let second = session.create("Second").expect("create");
    assert_eq!(
        session
            .settings()
            .expect("settings")
            .live_value("video.detail"),
        Some("high"),
        "a new profile starts from the declared default, not the last pilot's value"
    );
    session
        .set_setting("sound.device", "3")
        .expect("set on the second profile");
    session.commit().expect("commit");

    session.select(first).expect("select the first profile");
    let settings = session.settings().expect("settings");
    assert_eq!(settings.live_value("video.detail"), Some("low"));
    assert_eq!(
        settings.live_value("sound.device"),
        Some("0"),
        "and not the second profile's"
    );
    session.select(second).expect("select the second profile");
    assert_eq!(
        session
            .settings()
            .expect("settings")
            .live_value("sound.device"),
        Some("3")
    );
    session.finish().expect("teardown");
}

/// A catalog that could never recover is refused where it is built, and a key
/// or value a save could not hold never reaches storage.
#[test]
fn accept_f48_c_a_catalog_that_cannot_be_recovered_to_is_refused() {
    // A rule whose default is a value its own rule refuses could never recover
    // to anything, so it is refused at build time rather than at the moment a
    // player has an unusable display.
    assert!(
        SettingCatalog::new([SettingRule {
            key: "video.detail",
            apply: SettingApply::Live,
            value: ValueRule::Choice(&["low", "high"]),
            default: "ultra",
        }])
        .is_err(),
        "a default the rule refuses is refused"
    );
    // An empty value space is refused too.
    assert!(
        SettingCatalog::new([SettingRule {
            key: "video.detail",
            apply: SettingApply::Live,
            value: ValueRule::Choice(&[]),
            default: "low",
        }])
        .is_err()
    );
    // A key a save could not hold is refused when it is declared.
    assert!(
        SettingCatalog::new([SettingRule {
            key: "video detail",
            apply: SettingApply::Live,
            value: ValueRule::Choice(&["low", "high"]),
            default: "low",
        }])
        .is_err(),
        "a key that could not be written is refused"
    );

    // A value outside the save document's bounds is refused before it is
    // stored, so nothing unwritable is ever offered to the library.
    let base = TempBase::new("bounds");
    let mut session =
        ProfileSession::open_sandbox(base.path(), &catalog()).expect("open the sandbox");
    session.create("Pilot").expect("create");
    assert!(matches!(
        session.set_setting("video.detail", &"x".repeat(300)),
        Err(SessionError::Field(_))
    ));
    assert!(matches!(
        session.set_setting("video.detail", ""),
        Err(SessionError::Field(_))
    ));
    assert!(matches!(
        session.set_setting("../escape", "low"),
        Err(SessionError::Field(_))
    ));
    assert!(
        !session.has_uncommitted_settings(),
        "a refused key or value stores nothing"
    );
    session.finish().expect("teardown");
}

/// A stored setting whose apply label disagrees with the catalog keeps the
/// catalog's label, and a restart-required key is not put into force by a save
/// that claims it needed no restart.
#[test]
fn accept_f48_c_a_mislabeled_stored_setting_keeps_the_catalogs_label() {
    let base = TempBase::new("mislabeled");
    {
        let mut session =
            ProfileSession::open_sandbox(base.path(), &catalog()).expect("open the sandbox");
        session.create("Pilot").expect("create");
        // Write a restart-required setting straight into the save, labeled as
        // live, exactly as a mislabeled build would have left it.
        let mut document = session.document().expect("a document").clone();
        document.settings = vec![SettingEntry {
            key: "video.driver".to_owned(),
            apply: SettingApply::Live,
            value: "opengl".to_owned(),
        }];
        document.revision = document.revision.next().expect("a successor");
        session
            .library_mut()
            .save(&document)
            .expect("write the mislabeled save");
        session.finish().expect("teardown");
    }

    // A fresh session reads it through the catalog: the value is still stored,
    // but a device change a save claims needed no restart is not put into force,
    // and the mismatch is reported rather than accepted.
    let session =
        ProfileSession::open_sandbox(base.path(), &catalog()).expect("reopen the sandbox");
    let settings = session.settings().expect("settings");
    assert_eq!(
        settings.live_value("video.driver"),
        Some("d3d9"),
        "a restart-required key is not applied from a mislabeled save"
    );
    assert_eq!(
        settings.stored_value("video.driver"),
        Some("opengl"),
        "the stored value is kept, not rewritten behind the player's back"
    );
    let warnings = session.warnings();
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("the rule declares restart")),
        "the mismatch is visible: {warnings:?}"
    );
    assert!(
        settings.entries().iter().any(
            |entry| entry.key == "video.driver" && entry.apply == SettingApply::RestartRequired
        ),
        "and the catalog's label is what will be written"
    );
    session.finish().expect("teardown");
}

/// An uncommitted settings change is dropped at teardown and reported, never
/// written behind the caller's back (contract: "Persistent profile data receives
/// only an explicit outcome transaction").
#[test]
fn accept_f48_c_uncommitted_settings_are_dropped_and_reported_at_teardown() {
    let base = TempBase::new("dropped");
    {
        let mut session = production(&base).expect("open");
        session.create("Pilot").expect("create");
        session
            .set_setting("video.detail", "low")
            .expect("set a setting");
        assert!(session.has_uncommitted_settings());
        let report = session.finish().expect("teardown");
        assert!(
            report.uncommitted_changes,
            "a session that ends with uncommitted work says so"
        );
        assert_eq!(report.selected, Some(ProfileId::new(1).expect("nonzero")));
        assert_eq!(report.revision.map(|revision| revision.0), Some(1));
    }

    // Nothing was written: the profile still carries the declared default.
    let session = production(&base).expect("reopen");
    assert_eq!(
        session
            .settings()
            .expect("settings")
            .stored_value("video.detail"),
        Some("high"),
        "an uncommitted setting is dropped, not flushed"
    );
    let clean = session.finish().expect("teardown");
    assert!(
        !clean.uncommitted_changes,
        "a session that committed everything has nothing to report"
    );
}

/// Writes one new revision of a profile from a second owner of the same
/// directory — the concurrent-writer case the revision check exists for.
fn move_the_profile(directory: &Path, id: ProfileId, money_minor: u64) {
    let mut library =
        ProfileLibrary::open(directory.to_path_buf(), ProfileKind::Production).expect("library");
    let mut document = library
        .load(id)
        .expect("load")
        .document
        .expect("a stored revision");
    document.revision = document.revision.next().expect("a successor revision");
    document.campaign.money_minor = money_minor;
    library.save(&document).expect("the other writer commits");
}
