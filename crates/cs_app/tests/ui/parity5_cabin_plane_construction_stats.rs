//! Acceptance task PARITY5 P5-CONS-F (#2051): the Plane Construction hub's
//! statistics texts, bar graphs and plane picture. Task test prefix:
//! `accept_parity5_cabin_plane_construction_stats_`.
//!
//! * The synthetic tests run over the hub's layout text with the real 20 bar
//!   rows and invented art extents and strings; they prove the level
//!   function, the texts' wording, which bars light and which blueprint is
//!   shown, through the production page, shell and start flow. They prove
//!   nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the installation's own strings
//!   and the 11 blueprint arts. `#[ignore]`d because CI has no installation.

use std::collections::BTreeMap;

use bevy::app::App;
use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate};

use cs_app::ui::front_end::construction_stats::{
    self, AIRFRAMES, BAR_GROUPS, NO_ENGINE, NO_GUN, PlaneRecord, StatError, TEXT_ROWS,
};
use cs_app::ui::front_end::{
    HUB_SECTION, HubFeed, HubPage, MissionEnd, OriginalLayout, Screen, ShellResource, StartFlow,
    headless_app_construction,
};

use super::parity5_cabin_plane_construction_hub::{
    LAYOUT, TestInput, extents, ids, inject, name_page, names, open_hub,
};

/// The layout with every bar of the four groups, as the installation writes
/// them (the fixture carries four of the 20).
fn layout_text() -> String {
    let mut bars = String::new();
    for (group, x_set, y) in [
        ("OS", "BG1", "BG_Y1"),
        ("AD", "BG2", "BG_Y1"),
        ("SI", "BG1", "BG_Y2"),
        ("OC", "BG2", "BG_Y2"),
    ] {
        for bar in 1..=5 {
            bars.push_str(&format!(
                "PX_P_{group}{}=P,PX_BarGraph.png,<{x_set}_X{bar}>,<{y}>,<BG_Z>,1,0,1,0\n",
                bar - 1
            ));
        }
    }
    let mut out = String::new();
    for line in LAYOUT.lines() {
        if line.starts_with("PX_P_OS0")
            || line.starts_with("PX_P_AD4")
            || line.starts_with("PX_P_SI2")
            || line.starts_with("PX_P_OC0")
        {
            if line.starts_with("PX_P_OS0") {
                out.push_str(&bars);
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(&layout_text()).expect("the fixture parses")
}

/// The statistics' strings as the installation words them.
fn stat_ids(id: u32) -> Option<String> {
    Some(
        match id {
            93 => "[CSB11]",
            501 => "Poor",
            502 => "Fair",
            503 => "Average",
            504 => "Good",
            505 => "Excellent",
            1018 => "[CSB11]TOP SPEED: ",
            1019 => "[CSB11]ARMOR: ",
            1020 => "[CSB11]AGILITY: ",
            1132 => "[CSB11]OFFENSE: ",
            _ => return ids(id),
        }
        .to_owned(),
    )
}

fn blueprint_extents(art: &str) -> Option<(u32, u32)> {
    art.to_ascii_lowercase()
        .ends_with("_blueprint.tga")
        .then_some((200, 100))
}

fn page() -> HubPage {
    let mut page = HubPage::from_layout(&layout(), &names, &stat_ids).expect("the rows");
    page.set_blueprint_extents(
        (0..AIRFRAMES as u32)
            .filter_map(|n| {
                let art = cs_app::ui::front_end::construction_hangar::blueprint_art(n);
                let extent = blueprint_extents(&art)?;
                Some((art.to_ascii_lowercase(), extent))
            })
            .collect::<BTreeMap<_, _>>(),
    );
    page
}

/// A plane of the first airframe with the stock engine, no armor words, no
/// hardpoints and no guns.
fn plane() -> PlaneRecord {
    PlaneRecord {
        airframe: 0,
        engine: 1,
        armor: [0; 4],
        hardpoints: [0; 2],
        guns: [NO_GUN; 4],
        gun_pairs: 0,
    }
}

// ------------------------------------------------------- the synthetic half ---

/// The level function, by the engine's own arithmetic: speed
/// `trunc(200 * 1.0 - 1) / 85 = 2`, armor `(60 + 40 - 1) / 73 = 1`, agility
/// `(20 - 1) / 4 = 4` and offense `(2 * 480 + 480) / 2060 = 0`; and the table's
/// edges.
#[test]
fn accept_parity5_cabin_plane_construction_stats_a_the_levels_follow_the_engines_arithmetic() {
    let record = PlaneRecord {
        armor: [10; 4],
        hardpoints: [1, 1],
        guns: [2, NO_GUN, NO_GUN, NO_GUN],
        ..plane()
    };
    assert_eq!(construction_stats::levels(&record), Ok([2, 1, 4, 0]));
    // No engine: speed 0, whatever the airframe.
    let none = PlaneRecord {
        engine: NO_ENGINE,
        ..record
    };
    assert_eq!(construction_stats::levels(&none).map(|l| l[0]), Ok(0));
    // A big sum is stored as 4, never more.
    let heavy = PlaneRecord {
        airframe: 3,
        engine: 5,
        armor: [900; 4],
        hardpoints: [4, 4],
        ..plane()
    };
    let levels = construction_stats::levels(&heavy).expect("covered");
    // Offense is only (4 + 4) * 480 / 2060 = 1: no gun is fitted.
    assert_eq!((levels[0], levels[1], levels[3]), (4, 4, 1));
    // A pair doubles the slot's gun; the airframe's slot mask picks the column.
    let gun = PlaneRecord {
        airframe: 1,
        guns: [NO_GUN, NO_GUN, NO_GUN, 0],
        ..plane()
    };
    assert_eq!(construction_stats::gun_total(&gun), Ok(520));
    let pair = PlaneRecord {
        gun_pairs: 0b1000,
        ..gun
    };
    assert_eq!(construction_stats::gun_total(&pair), Ok(1040));
    // Nothing is guessed beyond the tables.
    let outside = PlaneRecord {
        airframe: AIRFRAMES as u32,
        ..plane()
    };
    assert_eq!(
        construction_stats::levels(&outside),
        Err(StatError::Airframe(AIRFRAMES as u32))
    );
    let engine = PlaneRecord {
        engine: 7,
        ..plane()
    };
    assert_eq!(
        construction_stats::levels(&engine),
        Err(StatError::Engine(7))
    );
}

/// The texts: head string, a space, rating string, in the rows the script
/// stores them in (speed, armor, agility, offense), and nothing without a
/// record.
#[test]
fn accept_parity5_cabin_plane_construction_stats_b_the_four_texts_name_their_rating() {
    let mut page = page();
    assert!(
        page.texts()
            .iter()
            .all(|text| !TEXT_ROWS.contains(&text.id.as_str())),
        "nothing is drawn without a record"
    );
    assert_eq!(page.unread_rows().len(), 4);
    page.set_feed(HubFeed {
        plane: Some(PlaneRecord {
            armor: [10; 4],
            hardpoints: [1, 1],
            guns: [2, NO_GUN, NO_GUN, NO_GUN],
            ..plane()
        }),
        ..HubFeed::default()
    });
    assert!(page.unread_rows().is_empty());
    let texts = page.texts();
    let text = |id: &str| {
        texts
            .iter()
            .find(|text| text.id == id)
            .map(|text| (text.text.clone(), text.size, text.rect.x, text.rect.y))
    };
    assert_eq!(
        text("PX_T_TOPSPEED"),
        Some(("TOP SPEED:  Average".to_owned(), Some(11), 14, 439))
    );
    assert_eq!(
        text("PX_T_ARMOR"),
        Some(("ARMOR:  Fair".to_owned(), Some(11), 14, 474))
    );
    assert_eq!(
        text("PX_T_AGILITY"),
        Some(("AGILITY:  Excellent".to_owned(), Some(11), 198, 438))
    );
    assert_eq!(
        text("PX_T_OFFENSE"),
        Some(("OFFENSE:  Poor".to_owned(), Some(11), 198, 474))
    );
}

fn cabin_app(page: HubPage) -> App {
    let mut shell = shell_for_layout();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let mut app = headless_app_construction(shell, Some(flow), name_page(), page);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn shell_for_layout() -> cs_app::ui::front_end::MenuShell {
    cs_app::ui::front_end::MenuShell::build(&layout(), &extents, &|_: &str| None)
}

fn hub_panes(app: &App) -> Vec<String> {
    app.world()
        .resource::<ShellResource>()
        .shell
        .sections()
        .get(&HUB_SECTION.to_ascii_uppercase())
        .expect("the section")
        .panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect()
}

/// The panes: without a record the backdrop alone, as before; with one, bar
/// `r` of a group is lit when `r <= level` (a level 0 still lights one bar) and
/// the airframe's blueprint sits behind them; a new record changes both.
#[test]
fn accept_parity5_cabin_plane_construction_stats_c_the_bars_and_the_blueprint_follow_the_record() {
    let mut app = cabin_app(page());
    open_hub(&mut app, "Ace");
    app.update();
    assert_eq!(hub_panes(&app), ["PX_P_BACKGROUND"]);
    let record = PlaneRecord {
        armor: [10; 4],
        hardpoints: [1, 1],
        guns: [2, NO_GUN, NO_GUN, NO_GUN],
        ..plane()
    };
    // Levels [2, 1, 4, 0]: speed (OS) 3 bars, armor (SI) 2, agility (AD) 5,
    // offense (OC) 1.
    assert!(
        app.world_mut()
            .resource_mut::<ShellResource>()
            .set_hub_feed(HubFeed {
                plane: Some(record),
                ..HubFeed::default()
            })
    );
    app.update();
    let panes = hub_panes(&app);
    assert_eq!(panes[..2], ["PX_P_BACKGROUND", "PX_P_PLANE0"]);
    let count = |group: &str| {
        panes
            .iter()
            .filter(|id| id.starts_with(group) && !id.starts_with("PX_P_PLANE"))
            .count()
    };
    assert_eq!(BAR_GROUPS.map(count), [3, 2, 5, 1], "bars lit: {panes:?}");
    let plane = app
        .world()
        .resource::<ShellResource>()
        .shell
        .sections()
        .get(&HUB_SECTION.to_ascii_uppercase())
        .and_then(|section| section.panes.iter().find(|pane| pane.id == "PX_P_PLANE0"))
        .cloned()
        .expect("the blueprint pane");
    assert_eq!(plane.art, "PX_0_Blueprint.tga");
    assert_eq!(
        (
            plane.rect.x,
            plane.rect.y,
            plane.rect.width,
            plane.rect.height
        ),
        (16, 44, 200, 100),
        "at PLANEX, PLANEY, the art's own extent"
    );
    // Another airframe, a stronger engine: other art, other levels.
    app.world_mut()
        .resource_mut::<ShellResource>()
        .set_hub_feed(HubFeed {
            plane: Some(PlaneRecord {
                airframe: 3,
                engine: 5,
                ..record
            }),
            ..HubFeed::default()
        });
    app.update();
    let panes = hub_panes(&app);
    assert_eq!(panes[1], "PX_P_PLANE0");
    let art = app
        .world()
        .resource::<ShellResource>()
        .shell
        .sections()
        .get(&HUB_SECTION.to_ascii_uppercase())
        .and_then(|section| section.panes.iter().find(|pane| pane.id == "PX_P_PLANE0"))
        .map(|pane| pane.art.clone());
    assert_eq!(art.as_deref(), Some("PX_3_Blueprint.tga"));
    // Taking the record away takes them off again.
    app.world_mut()
        .resource_mut::<ShellResource>()
        .set_hub_feed(HubFeed::default());
    app.update();
    assert_eq!(hub_panes(&app), ["PX_P_BACKGROUND"]);
}

// ------------------------------------------------------------ the retail half ---

/// **Retail.** The installation's strings word the four lines and all eleven
/// blueprint arts exist and are measured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_stats_retail_the_installations_strings_and_blueprints() {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let layout = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let mut page = HubPage::from_layout(
        &layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
    )
    .expect("the real hub rows read");
    for airframe in 0..AIRFRAMES as u32 {
        let art = cs_app::ui::front_end::construction_hangar::blueprint_art(airframe);
        let extent = screens.art_extent(&art);
        assert!(
            extent.is_some_and(|(w, h)| w > 0 && h > 0),
            "{art} is measured"
        );
    }
    page.set_feed(HubFeed {
        plane: Some(PlaneRecord {
            armor: [10; 4],
            hardpoints: [1, 1],
            guns: [2, NO_GUN, NO_GUN, NO_GUN],
            ..plane()
        }),
        ..HubFeed::default()
    });
    let texts: BTreeMap<String, String> = page
        .texts()
        .into_iter()
        .map(|text| (text.id, text.text))
        .collect();
    assert_eq!(texts["PX_T_TOPSPEED"], "TOP SPEED:  Average");
    assert_eq!(texts["PX_T_ARMOR"], "ARMOR:  Fair");
    assert_eq!(texts["PX_T_AGILITY"], "AGILITY:  Excellent");
    assert_eq!(texts["PX_T_OFFENSE"], "OFFENSE:  Poor");
}
