use std::collections::BTreeSet;

use cs_app::ui::scrapbook::{
    ScrapbookActionError, project, record_mission, record_stunt, replay_request, resolve_saved,
};
use cs_sim::campaign::Outcome;
use cs_sim::records::ScrapbookRecords;

use crate::*;

fn ids(records: &ScrapbookRecords) -> Vec<String> {
    project(&catalog(), records, &|t| Some(t.to_string()))
        .into_iter()
        .map(|p| p.id.key().to_owned())
        .collect()
}

#[test]
fn accept_f47_a_one_stunt_photo_unlocks_without_other_rewards() {
    let mut records = ScrapbookRecords::default();
    assert_eq!(ids(&records), vec!["intro"], "hidden pages stay hidden");
    assert!(record_stunt(&mut records, &stunt("a")));
    assert!(!record_stunt(&mut records, &stunt("a")), "idempotent");
    assert_eq!(ids(&records), vec!["intro", "photo-a"]);
}

#[test]
fn accept_f47_a_mission_success_unlocks_only_its_own_entries() {
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    assert_eq!(ids(&records), vec!["intro", "page-m1", "memento-m1"]);
}

#[test]
fn accept_f47_a_locked_visible_page_carries_no_artwork() {
    let pages = project(&catalog(), &ScrapbookRecords::default(), &|_| None);
    let intro = &pages[0];
    assert!(intro.unlocked);
    assert!(intro.image.is_some());
    assert_eq!(intro.title, None, "missing locale text is not invented");
    let mut shown_locked = entry(
        "late",
        EntryKind::Page,
        known(fact(UnlockFactKind::StuntCompleted, stunt("a"))),
    );
    shown_locked.visibility = EntryVisibility::Shown;
    let catalog = ScrapbookCatalog::new(vec![shown_locked]).unwrap();
    let page = &project(&catalog, &ScrapbookRecords::default(), &|_| None)[0];
    assert!(!page.unlocked);
    assert_eq!(page.image, None);
}

#[test]
fn accept_f47_a_saved_ids_resolve_in_every_locale() {
    let mut records = ScrapbookRecords::default();
    record_stunt(&mut records, &stunt("b"));
    let english = project(&catalog(), &records, &|t| Some(format!("en:{t}")));
    let swedish = project(&catalog(), &records, &|t| Some(format!("sv:{t}")));
    let saved: Vec<_> = english.iter().map(|p| p.id.clone()).collect();
    let catalog = catalog();
    let resolved = resolve_saved(&catalog, &saved);
    assert!(resolved.iter().all(Option::is_some));
    let titles = |pages: &[cs_app::ui::scrapbook::PageView]| -> Vec<Option<String>> {
        pages.iter().map(|p| p.title.clone()).collect()
    };
    assert_eq!(
        titles(&english),
        vec![
            Some("en:string_resource/intro-title".to_owned()),
            Some("en:string_resource/photo-b-title".to_owned()),
        ]
    );
    assert_eq!(
        titles(&swedish),
        vec![
            Some("sv:string_resource/intro-title".to_owned()),
            Some("sv:string_resource/photo-b-title".to_owned()),
        ]
    );
    let swedish_ids: Vec<_> = swedish.iter().map(|p| p.id.clone()).collect();
    assert_eq!(saved, swedish_ids, "locale never changes identity");
    let stale = resolve_saved(&catalog, &[item("removed-page")]);
    assert_eq!(stale, vec![None], "an unknown id resolves to nothing");
}

/// Sheet behavior 2: a reorder must not move an achievement between missions,
/// because identity is the stable id and not the declared position.
#[test]
fn accept_f47_a_reordering_the_catalog_moves_no_achievement() {
    let mut records = ScrapbookRecords::default();
    record_stunt(&mut records, &stunt("a"));
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    let declared: Vec<ScrapbookEntry> = catalog().entries().cloned().collect();
    let mut reversed = declared.clone();
    reversed.reverse();
    let flipped = ScrapbookCatalog::new(reversed).expect("catalog");

    let identity = |c: &ScrapbookCatalog| -> BTreeSet<(ContentId, EntryKind, bool)> {
        project(c, &records, &|t| Some(t.to_string()))
            .into_iter()
            .map(|p| (p.id, p.kind, p.unlocked))
            .collect()
    };
    assert_eq!(identity(&flipped), identity(&catalog()));
    // Every id still resolves to the same declaration after the reorder.
    for entry in &declared {
        assert_eq!(flipped.entry(&entry.id), Some(entry));
    }
    // Presentation order follows the declaration, identity does not.
    let visible = |c: &ScrapbookCatalog| -> Vec<ContentId> {
        project(c, &records, &|_| None)
            .into_iter()
            .map(|p| p.id)
            .collect()
    };
    assert_eq!(
        visible(&flipped),
        visible(&catalog()).into_iter().rev().collect::<Vec<_>>()
    );
}

#[test]
fn accept_f47_a_replay_request_names_mission_and_variant() {
    let mut records = ScrapbookRecords::default();
    let catalog = catalog();
    assert_eq!(
        replay_request(&catalog, &records, &item("page-m1")),
        Err(ScrapbookActionError::Locked(item("page-m1")))
    );
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    let request = replay_request(&catalog, &records, &item("page-m1")).unwrap();
    assert_eq!(request.mission, mission("m1"));
    assert_eq!(request.variant, Some(mission("m1-night")));
    record_mission(
        &mut records,
        &result(2, mission("m2"), Outcome::Succeeded, 5),
    )
    .unwrap();
    assert_eq!(
        replay_request(&catalog, &records, &item("page-m2")),
        Err(ScrapbookActionError::NoReplay(item("page-m2")))
    );
    assert_eq!(
        replay_request(&catalog, &records, &item("nope")),
        Err(ScrapbookActionError::UnknownEntry(item("nope")))
    );
}
