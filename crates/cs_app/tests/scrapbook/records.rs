use cs_app::ui::scrapbook::{project, record_mission};
use cs_sim::campaign::Outcome;
use cs_sim::records::{
    BestChange, BetterIs, DifficultyScope, RecordError, RecordKey, RecordReceipt, RecordRule,
    ScrapbookRecords,
};

use crate::*;

fn key(subject: cs_types::content::ContentId) -> RecordKey {
    RecordKey {
        subject,
        difficulty: None,
    }
}

fn titles(records: &ScrapbookRecords) -> Vec<String> {
    project(&catalog(), records, &|t| Some(t.to_string()))
        .into_iter()
        .map(|p| p.id.to_string())
        .collect()
}

#[test]
fn accept_f47_a_better_replay_updates_best_but_not_unrelated_progression() {
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 100),
    )
    .unwrap();
    record_mission(
        &mut records,
        &result(2, mission("m2"), Outcome::Succeeded, 70),
    )
    .unwrap();
    let m2_before = records.records.slot(&key(mission("m2"))).cloned();
    let pages_before = titles(&records);

    let receipt = record_mission(
        &mut records,
        &result(3, mission("m1"), Outcome::Succeeded, 250),
    )
    .unwrap();
    assert!(matches!(
        receipt,
        RecordReceipt::Applied {
            best: BestChange::Improved { previous: 100 },
            ..
        }
    ));
    let m1 = records.records.slot(&key(mission("m1"))).unwrap();
    assert_eq!((m1.best.score, m1.latest.score), (250, 250));
    // Nothing unrelated moved.
    assert_eq!(
        records.records.slot(&key(mission("m2"))).cloned(),
        m2_before
    );
    assert_eq!(titles(&records), pages_before);
    assert_eq!(records.records.len(), 2);
}

#[test]
fn accept_f47_a_worse_replay_keeps_best_and_updates_latest() {
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 250),
    )
    .unwrap();
    let receipt = record_mission(
        &mut records,
        &result(2, mission("m1"), Outcome::Succeeded, 40),
    )
    .unwrap();
    assert!(matches!(
        receipt,
        RecordReceipt::Applied {
            best: BestChange::NotBetter,
            ..
        }
    ));
    let slot = records.records.slot(&key(mission("m1"))).unwrap();
    assert_eq!((slot.best.score, slot.latest.score), (250, 40));
    assert_eq!(slot.best.outcome, outcome_id(1));
    assert_eq!(slot.latest.outcome, outcome_id(2));
}

#[test]
fn accept_f47_a_replayed_outcome_is_applied_once() {
    let mut records = ScrapbookRecords::default();
    let first = result(1, mission("m1"), Outcome::Succeeded, 100);
    record_mission(&mut records, &first).unwrap();
    record_mission(
        &mut records,
        &result(2, mission("m1"), Outcome::Succeeded, 50),
    )
    .unwrap();
    let before = records.clone();
    // The first result arrives again after a crash.
    let receipt = record_mission(&mut records, &first).unwrap();
    assert_eq!(receipt, RecordReceipt::AlreadyApplied);
    assert_eq!(records, before, "latest must not revert to the old run");
}

#[test]
fn accept_f47_a_tie_keeps_the_first_best_but_takes_latest() {
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 90),
    )
    .unwrap();
    let receipt = record_mission(
        &mut records,
        &result(2, mission("m1"), Outcome::Succeeded, 90),
    )
    .unwrap();
    assert!(matches!(
        receipt,
        RecordReceipt::Applied {
            best: BestChange::Tied,
            ..
        }
    ));
    let slot = records.records.slot(&key(mission("m1"))).unwrap();
    assert_eq!(slot.best.outcome, outcome_id(1));
    assert_eq!(slot.latest.outcome, outcome_id(2));
}

#[test]
fn accept_f47_a_lower_is_better_rule_and_difficulty_scope() {
    let rule = RecordRule {
        better: BetterIs::Lower,
        scope: DifficultyScope::PerDifficulty,
    };
    let mut records = ScrapbookRecords::default();
    let mut run = |serial, level: &str, score| {
        let mut r = result(serial, mission("m1"), Outcome::Succeeded, score);
        r.difficulty = difficulty(level);
        r.rule = rule;
        record_mission(&mut records, &r).unwrap()
    };
    run(1, "hard", 300);
    run(2, "easy", 500);
    let faster = run(3, "hard", 280);
    assert!(matches!(
        faster,
        RecordReceipt::Applied {
            best: BestChange::Improved { previous: 300 },
            ..
        }
    ));
    let slower = run(4, "hard", 900);
    assert!(matches!(
        slower,
        RecordReceipt::Applied {
            best: BestChange::NotBetter,
            ..
        }
    ));
    let hard = RecordKey {
        subject: mission("m1"),
        difficulty: Some(difficulty("hard")),
    };
    let easy = RecordKey {
        subject: mission("m1"),
        difficulty: Some(difficulty("easy")),
    };
    assert_eq!(records.records.slot(&hard).unwrap().best.score, 280);
    assert_eq!(records.records.slot(&easy).unwrap().best.score, 500);
}

#[test]
fn accept_f47_a_changed_rule_is_refused_without_change() {
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 10),
    )
    .unwrap();
    let before = records.clone();
    let mut other = result(2, mission("m1"), Outcome::Succeeded, 20);
    other.rule = RecordRule {
        better: BetterIs::Lower,
        scope: DifficultyScope::AllDifficulties,
    };
    assert!(matches!(
        record_mission(&mut records, &other),
        Err(RecordError::RuleChanged { .. })
    ));
    assert_eq!(records, before);
}

#[test]
fn accept_f47_a_failed_run_records_latest_but_unlocks_nothing() {
    let mut records = ScrapbookRecords::default();
    record_mission(&mut records, &result(1, mission("m1"), Outcome::Failed, 30)).unwrap();
    assert!(records.facts.is_empty());
    assert!(records.records.slot(&key(mission("m1"))).is_some());
    assert_eq!(titles(&records), vec!["scrapbook_item/intro"]);
}
