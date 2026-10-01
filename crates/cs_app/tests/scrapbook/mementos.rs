use cs_app::ui::scrapbook::{ScrapbookActionError, choose_memento, record_mission};
use cs_sim::campaign::Outcome;
use cs_sim::records::{MementoError, ScrapbookRecords};

use crate::*;

#[test]
fn accept_f47_a_memento_needs_an_unlocked_memento() {
    let catalog = catalog();
    let mut records = ScrapbookRecords::default();
    assert_eq!(
        choose_memento(&catalog, &mut records, &item("memento-m1")),
        Err(ScrapbookActionError::Memento(MementoError::Locked {
            memento: item("memento-m1")
        }))
    );
    assert_eq!(records.memento.chosen(), None);
    assert_eq!(
        choose_memento(&catalog, &mut records, &item("photo-a")),
        Err(ScrapbookActionError::NotAMemento(item("photo-a")))
    );
}

#[test]
fn accept_f47_a_memento_choice_survives_later_progress() {
    let catalog = catalog();
    let mut records = ScrapbookRecords::default();
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .unwrap();
    choose_memento(&catalog, &mut records, &item("memento-m1")).unwrap();
    record_mission(
        &mut records,
        &result(2, mission("m2"), Outcome::Succeeded, 9),
    )
    .unwrap();
    record_mission(&mut records, &result(3, mission("m1"), Outcome::Failed, 1)).unwrap();
    assert_eq!(records.memento.chosen(), Some(&item("memento-m1")));
}
