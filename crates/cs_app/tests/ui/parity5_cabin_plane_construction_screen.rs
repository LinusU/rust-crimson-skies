//! Acceptance task #2036: the Plane Construction screen the Plane Name
//! dialog's OK opens, walked end to end. Task test prefix:
//! `accept_parity5_cabin_plane_construction_screen_`.
//!
//! The pieces (hub, panels, purchase, hangar, feed) have their own prefixes;
//! these tests prove they are one screen on the production start flow: OK
//! opens the hub fed with the pilot's plane, Ready and Sell are their own
//! pages and not marked notices, and Cancel returns to the cabin. The
//! synthetic test runs over layout text in the original's format and proves
//! nothing about retail data; the `retail` test reads `$CS_GAME_DIR`.

use bevy::input::InputSystems;
use bevy::prelude::{App, IntoScheduleConfigs, PreUpdate};

use cs_app::ui::front_end::construction_hangar::HANGAR_SECTION;
use cs_app::ui::front_end::construction_purchase::PURCHASE_SECTION;
use cs_app::ui::front_end::{
    HubOutcome, MenuShell, MissionEnd, Screen, StartFlow, headless_app_construction,
};

use super::parity5_cabin_plane_construction_hangar::hangar_extents;
use super::parity5_cabin_plane_construction_hub::{
    TestInput, hub, inject, name_page, open_hub, press_button, section, state,
};
use super::parity5_cabin_plane_construction_purchase::{layout, purchase_ids, purchase_names};

fn screen_app() -> App {
    let layout = layout();
    let mut shell = MenuShell::build(&layout, &hangar_extents, &|_: &str| None);
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let page = cs_app::ui::front_end::HubPage::from_layout(&layout, &purchase_names, &purchase_ids)
        .expect("the fixture rows");
    let mut app = headless_app_construction(shell, Some(flow), name_page(), page);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

#[test]
fn accept_parity5_cabin_plane_construction_screen_ok_opens_the_fed_hub_and_ready_sell_cancel_are_real()
 {
    let mut app = screen_app();
    open_hub(&mut app, "Ace");
    let flow = state(&app).flow().expect("a flow");
    assert_eq!(flow.screen(), Screen::Construction);
    assert!(flow.hub_open());
    assert!(flow.notice().is_none(), "OK opens the screen, not a notice");
    // The pilot's plane and cash feed the hub on OK.
    assert!(hub(&app).feed().plane.is_some());
    assert_eq!(hub(&app).feed().cash, Some(0));
    assert_eq!(hub(&app).text(), "Ace");

    // Ready is the purchase panel, not READY_NOT_BUILT.
    press_button(&mut app, "PX_B_READY");
    assert!(hub(&app).purchase_open());
    assert_eq!(hub(&app).panel_section(), Some(PURCHASE_SECTION));
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(
        state(&app).hub_outcomes().last(),
        Some(&HubOutcome::Purchase)
    );

    // Sell is inactive until the hangar holds enough planes; it never puts
    // up the marked notice either way.
    press_button(&mut app, "PX_B_SELL");
    assert_ne!(section(&app), HANGAR_SECTION);
    assert!(state(&app).flow().expect("a flow").notice().is_none());

    // No page of the cabin path is still a marked stand-in.
    let flow = state(&app).flow().expect("a flow");
    for entry in flow.not_implemented_seen() {
        assert!(
            !matches!(entry.button, "PN_B_OK" | "PX_B_READY" | "PX_B_SELL"),
            "{entry:?} is still a marked notice"
        );
    }

    // Cancel leaves the screen for the cabin.
    press_button(&mut app, "PX_B_CANCEL");
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(state(&app).flow().expect("a flow").screen(), Screen::Cabin);
}

#[test]
fn accept_parity5_cabin_plane_construction_screen_the_marked_construction_notice_is_gone() {
    let source = include_str!("../../src/ui/front_end/start_flow.rs");
    assert!(!source.contains("CONSTRUCTION_NOT_BUILT"));
}
