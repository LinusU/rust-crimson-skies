//! Acceptance task #2076 (P5-FEED-C): the start flow keeps the pilot's saved
//! plane records and feeds Sell's count and the hangar list from them. Task
//! test prefix: `accept_parity5_cabin_plane_construction_feed_saved_`.
//!
//! Synthetic: the committed engine-facts table and a fixture layout. They
//! prove the wiring and the count rule (`E 2018`), not retail behaviour.

use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate};

use cs_app::engine_facts::engine_facts;
use cs_app::ui::front_end::pilot_plane::PilotPlane;
use cs_app::ui::front_end::{HubPage, MissionEnd, StartFlow, headless_app_construction};

use super::parity5_cabin_plane_construction_hangar as hangar_fx;
use super::parity5_cabin_plane_construction_hub as fx;

fn pilot() -> PilotPlane {
    let facts = engine_facts().expect("committed table").new_pilot();
    PilotPlane::from_facts(facts, false, true)
}

fn hub_page() -> HubPage {
    HubPage::from_layout(
        &hangar_fx::layout(),
        &hangar_fx::hangar_names,
        &hangar_fx::hangar_ids,
    )
    .expect("the fixture rows")
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_saved_a_a_new_pilot_holds_record_zero_only() {
    let plane = pilot();
    assert_eq!(plane.occupied(), 1);
    // On the construction screen one record stands for the plane being built.
    assert_eq!(plane.saved_count(), 0);
    assert_eq!(plane.selected(), 0);
    let planes = plane.hangar_planes();
    assert_eq!(planes.len(), 25);
    let first = planes[0].as_ref().expect("record 0 is occupied");
    assert_eq!((first.kind, first.airframe), (1, 5));
    assert!(planes[1..].iter().all(Option::is_none));
    // The stock template survives an emptied working plane.
    let bare = PilotPlane::from_facts(engine_facts().expect("table").new_pilot(), false, false);
    assert_eq!(bare.hangar_planes()[0], planes[0]);
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_saved_b_sell_needs_the_engines_minimum() {
    let mut plane = pilot();
    let mut page = hub_page();
    plane.feed_hub(&mut page);
    assert_eq!(page.feed().saved_planes, Some(0));
    assert!(!page.sell_enabled());
    // Records 0..=2 occupied: three less the plane being built is two.
    for slot in 1..3 {
        plane.save_plane(slot, "Extra", plane.working_words().to_vec());
    }
    plane.feed_hub(&mut page);
    assert_eq!(page.feed().saved_planes, Some(2));
    assert!(!page.sell_enabled());
    plane.save_plane(3, "Extra", plane.working_words().to_vec());
    plane.feed_hub(&mut page);
    assert_eq!(page.feed().saved_planes, Some(3));
    assert!(page.sell_enabled());
    assert_eq!(page.hangar().expect("hangar").saved_count(), 3);
    // A sale empties a record and Sell goes inactive again.
    plane.clear_saved(3);
    plane.feed_hub(&mut page);
    assert!(!page.sell_enabled());
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_saved_c_the_hangar_list_shows_the_pilots_planes() {
    let mut plane = pilot();
    plane.save_plane(4, "Raven", plane.working_words().to_vec());
    // While constructing, the walk returns the selected record when it
    // reaches it; selecting an empty one leaves the other rows in order.
    plane.select(10);
    let mut page = hub_page();
    plane.feed_hub(&mut page);
    let hangar = page.hangar().expect("hangar");
    // The walk skips empty records: row 0 is record 0, row 1 is record 4.
    let row = hangar.plane_at(1).expect("a second row");
    assert_eq!((row.name.as_str(), row.airframe), ("Raven", 5));
    assert!(row.value > 0);
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_saved_d_the_plane_name_ok_feeds_the_saved_planes() {
    let mut shell = fx::shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    let mut app = headless_app_construction(shell, Some(flow), fx::name_page(), hub_page());
    app.init_resource::<fx::TestInput>();
    app.add_systems(PreUpdate, fx::inject.after(InputSystems));
    app.update();
    assert_eq!(fx::hub(&app).feed().saved_planes, None, "unknown before OK");
    fx::open_hub(&mut app, "Ace");
    let hub = fx::hub(&app);
    assert_eq!(hub.feed().saved_planes, Some(0));
    assert!(!hub.sell_enabled());
    assert!(hub.hangar().expect("hangar").plane_at(0).is_some());
}
