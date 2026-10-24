//! Acceptance task #2075 (P5-FEED-B): the start flow keeps a per-pilot
//! working plane and cash and feeds the Plane Construction hub on the Plane
//! Name dialog's OK. Task test prefix:
//! `accept_parity5_cabin_plane_construction_feed_`.
//!
//! The synthetic tests run in CI over the committed engine-facts table and a
//! fixture layout; they prove the wiring and the arithmetic, not retail
//! behaviour. The retail test reads the installation's airframe strings.

use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate};

use cs_app::engine_facts::engine_facts;
use cs_app::ui::front_end::construction_purchase::{capacity, plane_value, plane_weight};
use cs_app::ui::front_end::construction_stats::{NO_ENGINE, NO_GUN};
use cs_app::ui::front_end::pilot_plane::{PilotPlane, airframe_names};
use cs_app::ui::front_end::{HubPage, MissionEnd, Screen, StartFlow, headless_app_construction};

use super::parity5_cabin_plane_construction_hub as fx;

fn facts_plane(default_loadout: bool) -> PilotPlane {
    let facts = engine_facts().expect("committed table").new_pilot();
    PilotPlane::from_facts(facts, false, default_loadout)
}

fn ids(id: u32) -> Option<String> {
    match id {
        3005 => Some("[COUR9]Devastator".to_owned()),
        3025 => Some("[COUR9]Dev".to_owned()),
        other => fx::ids(other),
    }
}

fn cabin_app() -> bevy::prelude::App {
    let mut shell = fx::shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let hub = HubPage::from_layout(&fx::layout(), &fx::names, &ids).expect("the fixture rows");
    let mut app = headless_app_construction(shell, Some(flow), fx::name_page(), hub);
    app.init_resource::<fx::TestInput>();
    app.add_systems(PreUpdate, fx::inject.after(InputSystems));
    app.update();
    app
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_a_a_new_pilot_starts_with_the_template_and_no_cash()
{
    let plane = facts_plane(true);
    assert_eq!(plane.cash(), 0);
    let record = plane.record().expect("the template reads");
    assert_eq!((record.airframe, record.engine), (5, 1));
    assert_eq!(record.hardpoints, [2, 2]);
    assert_eq!(record.armor, [25; 4]);
    assert_eq!(record.guns, [2, 1, 0, NO_GUN]);
    assert_eq!(record.gun_pairs, 7);
    // The unchecked default-loadout box empties it (`E 2212` with flag 0).
    let empty = facts_plane(false)
        .record()
        .expect("the emptied record reads");
    assert_eq!(empty.guns, [NO_GUN; 4]);
    assert_eq!(empty.engine, NO_ENGINE);
    assert_eq!(empty.hardpoints, [0, 0]);
    assert_eq!(empty.gun_pairs, 0);
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_b_the_feed_is_the_engine_tables_figures() {
    let plane = facts_plane(true);
    let record = plane.record().expect("the template reads");
    let names = airframe_names(&ids);
    assert_eq!(names.len(), 11);
    assert_eq!(names[5].long.as_deref(), Some("Devastator"));
    let feed = plane.hub_feed(&names);
    assert_eq!(feed.cash, Some(0));
    assert_eq!(feed.plane_cost, Some(plane_value(&record).unwrap() as u64));
    assert_eq!(
        feed.weight_capacity,
        Some(capacity(&record).unwrap() as u64)
    );
    assert_eq!(
        feed.current_weight,
        Some(plane_weight(&record).unwrap() as u64)
    );
    assert_eq!(feed.airframe.as_deref(), Some("Devastator"));
    assert_eq!(feed.airframe_short.as_deref(), Some("Dev"));
    assert!(!feed.dirty);
    assert_eq!(feed.plane, Some(record));
    let purchase = plane.purchase_feed().expect("a priced record");
    assert_eq!((purchase.record, purchase.cash), (record, 0));
    // A plane that changes is priced again.
    let mut other = plane.clone();
    other.set_airframe(1);
    assert_ne!(other.hub_feed(&names).plane_cost, feed.plane_cost);
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_c_the_plane_name_ok_feeds_the_hub() {
    let mut app = cabin_app();
    assert_eq!(fx::hub(&app).feed().plane, None, "nothing is fed before OK");
    fx::open_hub(&mut app, "Ace");
    let feed = fx::hub(&app).feed().clone();
    let expected = facts_plane(true);
    assert_eq!(feed, expected.hub_feed(fx::hub(&app).airframe_names()));
    assert_eq!(feed.cash, Some(0));
    assert_eq!(feed.airframe.as_deref(), Some("Devastator"));
    let record = feed.plane.expect("the working plane is fed");
    assert_eq!(record.airframe, 5);
    // The hub draws the figures.
    let texts = fx::hub(&app).texts();
    let cost = texts
        .iter()
        .find(|text| text.id == "PX_T_PLANECOST")
        .expect("cost line");
    assert_eq!(
        cost.text,
        format!("PLANE COST:  ${}", plane_value(&record).unwrap())
    );
    let airframe = texts
        .iter()
        .find(|text| text.id == "PX_T_AIRFRAME")
        .expect("airframe line");
    assert_eq!(airframe.text, "AIRFRAME: Devastator");
}

#[test]
fn accept_parity5_cabin_plane_construction_feed_d_the_unchecked_default_loadout_feeds_the_emptied_plane()
 {
    let mut app = cabin_app();
    fx::press_button(&mut app, "PC_B_PLANEX");
    fx::type_text(&mut app, "Bare");
    fx::press_button(&mut app, "PN_B_DEFAULT");
    fx::press_button(&mut app, cs_app::ui::front_end::plane_name::OK_BUTTON);
    let record = fx::hub(&app)
        .feed()
        .plane
        .expect("the working plane is fed");
    assert_eq!(record.guns, [NO_GUN; 4]);
    assert_eq!(record.engine, NO_ENGINE);
}

/// **Retail.** The installation's airframe strings (`3000 + n`, `3020 + n`)
/// name every airframe the feed can show.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_feed_retail_the_airframe_strings_name_the_start_plane() {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let names = airframe_names(&|id: u32| strings.resolve_id_tagged(id));
    assert_eq!(names.len(), 11);
    for (n, name) in names.iter().enumerate() {
        assert!(
            name.long.as_deref().is_some_and(|text| !text.is_empty()),
            "3000+{n}"
        );
        assert!(
            name.short.as_deref().is_some_and(|text| !text.is_empty()),
            "3020+{n}"
        );
    }
    let feed = facts_plane(true).hub_feed(&names);
    assert_eq!(feed.airframe, names[5].long);
    assert_eq!(feed.airframe_short, names[5].short);
}
