//! Task #1669 `M01-LC-FC-A`: the Flight Check view model read from the
//! mission's own data. Task test prefix: `accept_m01_lc_fc_a_`.
//!
//! The fixture tests prove nothing is hard-coded: another mission's strings,
//! objectives and airframes give another view. The retail test reads M01
//! (`zbd/c3/m01`) and pins its title, its four objective rows and that every
//! button's hotspot is a row of the original `[FlightCheck]` section. No
//! original text is written into this file: it is compared against what the
//! production catalog resolves, except the long name the task states.

use std::path::PathBuf;

use cs_app::mission_start::{AIRFRAME_TABLE, CAMPAIGN_AIRFRAME_ROW};
use cs_app::ui::front_end::{
    Entry, FlightCheckButton, FlightCheckSlot, FlightCheckView, FrontEndScreens, OriginalLayout,
    objective_keys,
};
use cs_content::objectives_engine::{
    EngineDeclaration, EngineIdentity, EngineObjective, EngineObjectives,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn airframe(scene_root: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, scene_root).expect("valid id"),
        Provenance::designed(ClaimId::new("m01lcfca.test").expect("valid claim")),
    ))
}

fn engine(identities: &[(&str, u32, Option<&str>)]) -> EngineObjectives {
    let objectives = identities
        .iter()
        .map(|(role, ordinal, message)| {
            EngineObjective::loaded(EngineDeclaration {
                identity: Some(EngineIdentity::from_record(
                    role,
                    *ordinal,
                    message.map(str::to_owned),
                )),
                ..EngineDeclaration::default()
            })
        })
        .collect();
    EngineObjectives::new(objectives).expect("no targets to range-check")
}

fn view(
    title: &str,
    identities: &[(&str, u32, Option<&str>)],
    texts: &[(&str, &str)],
    pilot: &str,
    wingman: Resolved<ContentId>,
) -> FlightCheckView {
    let keys = objective_keys(&engine(identities));
    FlightCheckView::build(
        Entry::Known(title.to_owned()),
        &keys,
        |key| {
            texts
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, text)| (*text).to_owned())
                .ok_or_else(|| "no such row".to_owned())
        },
        &airframe(pilot),
        &[wingman],
    )
}

#[test]
fn accept_m01_lc_fc_a_fixture_other_strings_give_other_text() {
    let one = view(
        "Alpha - One",
        &[("PRIMARY", 2, Some("MSG_B")), ("PRIMARY", 1, Some("MSG_A"))],
        &[("MSG_A", "first"), ("MSG_B", "second")],
        "player_kestrel",
        airframe("player_fury"),
    );
    let two = view(
        "Beta - Two",
        &[("PRIMARY", 1, Some("MSG_X")), ("SECONDARY", 1, None)],
        &[("MSG_X", "other")],
        "player_pfighter",
        airframe("player_warhawk"),
    );

    assert_eq!(one.title, Entry::Known("Alpha - One".to_owned()));
    let texts: Vec<_> = one.objectives.iter().map(|row| row.text.clone()).collect();
    assert_eq!(
        texts,
        [
            Entry::Known("first".to_owned()),
            Entry::Known("second".to_owned())
        ],
        "objectives follow (role, ordinal), not declaration order"
    );
    assert_eq!(one.pilot.plane, Entry::Known("Kestrel".to_owned()));
    assert_eq!(one.wingman.plane, Entry::Known("Fury".to_owned()));

    assert_eq!(two.title, Entry::Known("Beta - Two".to_owned()));
    assert_eq!(
        two.objectives.len(),
        1,
        "a message-less identity has no row"
    );
    assert_eq!(two.objectives[0].text, Entry::Known("other".to_owned()));
    assert_eq!(two.pilot.plane, Entry::Known("Devastator".to_owned()));
    assert_eq!(two.wingman.plane, Entry::Known("Warhawk".to_owned()));
}

#[test]
fn accept_m01_lc_fc_a_fixture_build_leaves_the_loadout_unknown_with_reason_and_task() {
    let v = view(
        "T",
        &[("PRIMARY", 1, Some("MSG_A"))],
        &[],
        "player_kestrel",
        airframe("player_fury"),
    );
    for slot in [&v.pilot, &v.wingman] {
        for list in [&slot.guns, &slot.rockets] {
            match list {
                Entry::Unknown {
                    reason,
                    resolving_task,
                } => {
                    assert!(!reason.is_empty() && !resolving_task.is_empty());
                }
                Entry::Known(_) => panic!("the loadout is not measured"),
            }
        }
    }
    assert_eq!(v.pilot.slot, FlightCheckSlot::Player);
    assert_eq!(v.wingman.slot, FlightCheckSlot::Wingmate);
    assert!(
        matches!(&v.objectives[0].text, Entry::Unknown { reason, .. } if reason.contains("MSG_A")),
        "an unresolved key is unknown and names the key"
    );
}

#[test]
fn accept_m01_lc_fc_a_fixture_unbound_wingman_is_unknown() {
    let wingman = Resolved::unknown(
        ClaimId::new("m01lcfca.open").expect("valid claim"),
        "not measured",
    )
    .expect("reason");
    let v = view("T", &[], &[], "player_kestrel", wingman);
    assert!(matches!(&v.wingman.plane, Entry::Unknown { reason, .. } if reason == "not measured"));
    assert!(v.objectives.is_empty());
    let buttons: Vec<_> = v.buttons().iter().map(|b| b.hotspot()).collect();
    assert_eq!(
        buttons,
        [
            "FC_B_CHANGEAMMO",
            "FC_B_CHANGEAMMOW",
            "FC_B_RETURNBRIEF",
            "FC_B_FLYMISSION"
        ]
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_fc_a_retail_m01_title_objectives_and_hotspots() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let v = FlightCheckView::from_retail(&root, "zbd/c3/m01").expect("M01 reads");

    assert_eq!(
        v.title,
        Entry::Known("Hawaii - The Lost Treasure of Sir Francis Drake".to_owned())
    );
    assert_eq!(v.objectives.len(), 4, "M01 declares four briefing messages");
    for (index, row) in v.objectives.iter().enumerate() {
        assert!(row.key.starts_with("MSG_BRF_HAM1_OBJ"), "{}", row.key);
        assert!(row.key.ends_with(&(index + 1).to_string()), "{}", row.key);
        let text = row
            .text
            .known()
            .unwrap_or_else(|| panic!("{} resolves", row.key));
        assert!(!text.is_empty());
    }
    assert_eq!(
        v.pilot.plane,
        Entry::Known(
            AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW]
                .display_name
                .to_owned()
        )
    );
    // The lists are the measured fresh-campaign defaults (M01-LC-FC-B): the
    // Devastator record's six gun lines and four rocket lines, the same for
    // the wingman, every line numbered and labelled from the string table.
    for slot in [&v.pilot, &v.wingman] {
        let guns = slot.guns.known().expect("the gun list reads");
        let rockets = slot.rockets.known().expect("the rocket list reads");
        assert_eq!(guns.len(), 6, "{guns:?}");
        assert_eq!(rockets.len(), 4, "{rockets:?}");
        for (index, line) in guns.iter().enumerate() {
            assert!(line.contains(&(index + 1).to_string()), "{line}");
            assert!(!line.contains('%'), "{line}");
        }
        assert!(rockets[2].contains('5'), "{:?}", rockets[2]);
    }
    assert_eq!(v.pilot.guns, v.wingman.guns);

    // Every button's hotspot is a row of the original FlightCheck section.
    let screens = FrontEndScreens::open(&root).expect("the screen container opens");
    let (text, _) = screens.layout_text().expect("the layout reads");
    let layout = OriginalLayout::parse(&text).expect("the layout parses");
    let section = layout.section("FlightCheck").expect("FlightCheck section");
    for button in FlightCheckButton::ALL {
        assert!(
            section
                .buttons
                .iter()
                .any(|row| row.id.eq_ignore_ascii_case(button.hotspot())),
            "{} is not a FlightCheck hotspot",
            button.hotspot()
        );
    }
}
