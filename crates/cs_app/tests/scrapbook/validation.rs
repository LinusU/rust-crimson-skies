use cs_app::ui::scrapbook::{project, record_mission};
use cs_content::scrapbook::{MAX_UNLOCK_DEPTH, ScrapbookError};
use cs_sim::campaign::Outcome;
use cs_sim::records::ScrapbookRecords;
use cs_types::content::{ContentKind, Resolved};

use crate::*;

#[test]
fn accept_f47_a_unknown_rule_never_unlocks() {
    let unknown = Resolved::unknown(
        ClaimId::new("f47a.unknown-rule").unwrap(),
        "original unlock rule not decoded",
    )
    .unwrap();
    let catalog = ScrapbookCatalog::new(vec![entry("mystery", EntryKind::Page, unknown)]).unwrap();
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    assert!(project(&catalog, &records, &|_| None).is_empty());
}

#[test]
fn accept_f47_a_all_any_predicates_evaluate_against_facts() {
    let both = Unlock::All(vec![
        fact(UnlockFactKind::MissionSucceeded, mission("m1")),
        Unlock::Any(vec![
            fact(UnlockFactKind::StuntCompleted, stunt("a")),
            fact(UnlockFactKind::StuntCompleted, stunt("b")),
        ]),
    ]);
    let catalog =
        ScrapbookCatalog::new(vec![entry("combo", EntryKind::Page, known(both))]).unwrap();
    let mut records = ScrapbookRecords::default();
    let shown = |r: &ScrapbookRecords| project(&catalog, r, &|_| None).len();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    assert_eq!(shown(&records), 0);
    cs_app::ui::scrapbook::record_stunt(&mut records, &stunt("b"));
    assert_eq!(shown(&records), 1);
}

#[test]
fn accept_f47_a_catalog_rejects_malformed_entries() {
    let page = || entry("p", EntryKind::Page, known(Unlock::Always));
    assert!(matches!(
        ScrapbookCatalog::new(vec![page(), page()]),
        Err(ScrapbookError::DuplicateEntry { .. })
    ));
    let mut wrong_title = page();
    wrong_title.title = mission("x");
    assert!(matches!(
        ScrapbookCatalog::new(vec![wrong_title]),
        Err(ScrapbookError::WrongKind { role: "title", .. })
    ));
    let wrong_subject = entry(
        "w",
        EntryKind::Page,
        known(fact(UnlockFactKind::StuntCompleted, mission("m1"))),
    );
    assert!(matches!(
        ScrapbookCatalog::new(vec![wrong_subject]),
        Err(ScrapbookError::WrongFactSubject {
            expected: ContentKind::Stunt,
            ..
        })
    ));
    let empty = entry("e", EntryKind::Page, known(Unlock::Any(vec![])));
    assert!(matches!(
        ScrapbookCatalog::new(vec![empty]),
        Err(ScrapbookError::EmptyCombinator { .. })
    ));
    let mut deep = Unlock::Always;
    for _ in 0..=MAX_UNLOCK_DEPTH {
        deep = Unlock::All(vec![deep]);
    }
    assert!(matches!(
        ScrapbookCatalog::new(vec![entry("d", EntryKind::Page, known(deep))]),
        Err(ScrapbookError::UnlockTooDeep { .. })
    ));
    let mut at_limit = Unlock::Always;
    for _ in 0..MAX_UNLOCK_DEPTH {
        at_limit = Unlock::All(vec![at_limit]);
    }
    assert!(
        ScrapbookCatalog::new(vec![entry("ok", EntryKind::Page, known(at_limit))]).is_ok(),
        "the deepest accepted nesting is {MAX_UNLOCK_DEPTH}"
    );
    let mut memento = entry("m", EntryKind::Memento, known(Unlock::Always));
    memento.replay = Some(ReplayLink {
        mission: mission("m1"),
        variant: None,
    });
    assert!(matches!(
        ScrapbookCatalog::new(vec![memento]),
        Err(ScrapbookError::MementoWithReplay { .. })
    ));
}
