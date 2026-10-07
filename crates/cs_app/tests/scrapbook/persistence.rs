//! Acceptance stage F47-B: idempotent record and memento persistence in the
//! selected profile. All data is authored synthetic data in a temporary
//! directory; this proves the write path, never an original scrapbook rule.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::ProfileSession;
use cs_app::ui::scrapbook::{
    PersistError, Persisted, ScrapbookActionError, load, persist_memento, persist_mission,
    persist_stunt, project, stored,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::Outcome;
use cs_sim::records::{
    BestChange, BetterIs, DifficultyScope, FIELD_PREFIX, MementoError, RecordKey, RecordReceipt,
    RecordRule, RestoreError, ScrapbookRecords,
};
use cs_types::profile::ExtraField;

use crate::*;

struct TempBase(PathBuf);

impl TempBase {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f47-b-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture base");
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

fn open(base: &TempBase) -> ProfileSession {
    ProfileSession::open_sandbox(base.path(), &SettingCatalog::new([]).expect("catalog"))
        .expect("open sandbox")
}

/// A sandbox with one created profile.
fn fresh(base: &TempBase) -> ProfileSession {
    let mut session = open(base);
    session.create("Pilot").expect("create");
    session
}

fn revision(session: &ProfileSession) -> u64 {
    session.document().expect("document").revision.0
}

fn titles(records: &ScrapbookRecords) -> Vec<String> {
    project(&catalog(), records, &|t| Some(t.to_string()))
        .into_iter()
        .map(|page| page.id.key().to_owned())
        .collect()
}

#[test]
fn accept_f47_b_one_stunt_photo_persists_without_other_mission_rewards() {
    let base = TempBase::new();
    let before;
    {
        let mut session = fresh(&base);
        before = revision(&session);
        assert_eq!(
            persist_stunt(&mut session, &stunt("a")).expect("persist"),
            Persisted::Written(())
        );
        assert_eq!(revision(&session), before + 1);
    }
    let session = open(&base);
    let records = stored(&session).expect("stored");
    assert_eq!(titles(&records), ["intro", "photo-a"]);
    assert!(!records.facts.is_empty());
    assert_eq!(records.facts.len(), 1);
}

#[test]
fn accept_f47_b_repeating_a_stunt_writes_nothing() {
    let base = TempBase::new();
    let mut session = fresh(&base);
    persist_stunt(&mut session, &stunt("a")).expect("first");
    let after_first = revision(&session);
    assert_eq!(
        persist_stunt(&mut session, &stunt("a")).expect("second"),
        Persisted::AlreadyApplied
    );
    assert_eq!(revision(&session), after_first);
}

#[test]
fn accept_f47_b_replayed_mission_result_is_written_once_and_best_survives() {
    let base = TempBase::new();
    let mut session = fresh(&base);
    session
        .commit_with(|document| {
            document.campaign.money_minor = 700;
            Ok(())
        })
        .expect("progress");
    let first = result(1, mission("m1"), Outcome::Succeeded, 10);
    let receipt = persist_mission(&mut session, &first).expect("first");
    assert!(matches!(
        receipt,
        Persisted::Written(RecordReceipt::Applied {
            best: BestChange::First,
            ..
        })
    ));
    let revision_after_first = revision(&session);
    // The crash-before-acknowledgment case: the same outcome id arrives again.
    assert_eq!(
        persist_mission(&mut session, &first).expect("replay"),
        Persisted::AlreadyApplied
    );
    assert_eq!(revision(&session), revision_after_first);

    // A worse run replaces latest only; a better run raises best.
    persist_mission(&mut session, &result(2, mission("m1"), Outcome::Failed, 3)).expect("worse");
    persist_mission(
        &mut session,
        &result(3, mission("m1"), Outcome::Succeeded, 20),
    )
    .expect("better");
    persist_mission(
        &mut session,
        &result(4, mission("m1"), Outcome::Succeeded, 15),
    )
    .expect("middling");

    drop(session);
    let session = open(&base);
    let records = stored(&session).expect("stored");
    let key = RecordKey {
        subject: mission("m1"),
        difficulty: None,
    };
    let slot = records.records.slot(&key).expect("slot");
    assert_eq!(slot.best.score, 20);
    assert_eq!(slot.best.outcome, outcome_id(3));
    assert_eq!(slot.latest.score, 15);
    assert_eq!(slot.latest.outcome, outcome_id(4));
    // Unrelated progression was read, not rewritten.
    let document = session.document().expect("document");
    assert_eq!(document.campaign.money_minor, 700);
    // The replay after the reload is still recognised.
    let mut session = session;
    assert_eq!(
        persist_mission(
            &mut session,
            &result(3, mission("m1"), Outcome::Succeeded, 99)
        )
        .expect("replay after restart"),
        Persisted::AlreadyApplied
    );
}

#[test]
fn accept_f47_b_memento_choice_persists_independently_of_campaign() {
    let base = TempBase::new();
    let mut session = fresh(&base);
    let catalog = catalog();
    let locked = persist_memento(&mut session, &catalog, &item("memento-m1"));
    assert!(matches!(
        locked,
        Err(PersistError::Action(ScrapbookActionError::Memento(
            MementoError::Locked { .. }
        )))
    ));
    let untouched = revision(&session);
    persist_mission(
        &mut session,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .expect("m1");
    assert!(revision(&session) > untouched);
    assert_eq!(
        persist_memento(&mut session, &catalog, &item("memento-m1")).expect("choose"),
        Persisted::Written(())
    );
    let chosen_at = revision(&session);
    assert_eq!(
        persist_memento(&mut session, &catalog, &item("memento-m1")).expect("again"),
        Persisted::AlreadyApplied
    );
    assert_eq!(revision(&session), chosen_at);

    // Later progression, and a campaign write, leave the choice alone.
    persist_mission(&mut session, &result(2, mission("m2"), Outcome::Failed, 1)).expect("m2");
    session
        .commit_with(|document| {
            document.campaign.money_minor = 1;
            Ok(())
        })
        .expect("campaign write");
    drop(session);
    let session = open(&base);
    assert_eq!(
        stored(&session).expect("stored").memento.chosen(),
        Some(&item("memento-m1"))
    );
}

#[test]
fn accept_f47_b_difficulty_scoped_lower_is_better_rules_round_trip() {
    let mut records = ScrapbookRecords::default();
    let lower = RecordRule {
        better: BetterIs::Lower,
        scope: DifficultyScope::PerDifficulty,
    };
    for (serial, name, score) in [(1, "easy", 90), (2, "hard", 70), (3, "easy", 80)] {
        let mut run = result(serial, mission("race"), Outcome::Succeeded, score);
        run.difficulty = difficulty(name);
        run.rule = lower;
        cs_app::ui::scrapbook::record_mission(&mut records, &run).expect("record");
    }
    let fields = records.to_fields();
    assert!(fields.iter().all(|(key, _)| key.starts_with(FIELD_PREFIX)));
    let restored =
        ScrapbookRecords::from_fields(fields.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .expect("restore");
    assert_eq!(restored, records);
    assert_eq!(restored.to_fields(), fields);
}

#[test]
fn accept_f47_b_unreadable_stored_scrapbook_is_not_written_over() {
    let base = TempBase::new();
    let mut session = fresh(&base);
    session
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "scrapbook.fact.0".to_owned(),
                value: "mission not-an-id".to_owned(),
            });
            Ok(())
        })
        .expect("plant a bad field");
    let planted = revision(&session);
    assert!(matches!(
        persist_stunt(&mut session, &stunt("a")),
        Err(PersistError::Restore(RestoreError::Malformed { .. }))
    ));
    assert_eq!(revision(&session), planted);
    assert!(load(session.document().expect("document")).is_err());

    let unknown = [("scrapbook.mystery", "x")];
    assert!(matches!(
        ScrapbookRecords::from_fields(unknown),
        Err(RestoreError::UnknownField { .. })
    ));
}

#[test]
fn accept_f47_b_other_extra_fields_survive_a_scrapbook_write() {
    let base = TempBase::new();
    let mut session = fresh(&base);
    session
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "other.thing".to_owned(),
                value: "kept".to_owned(),
            });
            Ok(())
        })
        .expect("foreign field");
    persist_stunt(&mut session, &stunt("a")).expect("a");
    persist_stunt(&mut session, &stunt("b")).expect("b");
    let extra = &session.document().expect("document").extra;
    assert!(
        extra
            .iter()
            .any(|f| f.key == "other.thing" && f.value == "kept")
    );
    assert_eq!(
        extra
            .iter()
            .filter(|f| f.key.starts_with(FIELD_PREFIX))
            .count(),
        2
    );
}
