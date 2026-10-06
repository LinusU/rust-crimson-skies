//! M01-LC-PLAYER-AIRFRAME-SOURCE (#715): where the original assigns the
//! player's airframe, and what of the start pose is measured.
//!
//! The measurement and its evidence are in
//! `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`. In short:
//! the executable holds an eleven-row airframe table with one name-to-index
//! routine, the only document key that names the player's airframe is
//! `player_plane` and it is read only by the instant-action setup from
//! `ia.zrd`, campaign mission data carries no such key, and the installation
//! holds no profile or hangar file — so M01's airframe stays a named unknown
//! (AGENTS.md rule 4) while the table, the key and the metre unit of the
//! stored position are bound here.
//!
//! Nothing is `verified_original`: no original run happened.

use std::path::{Path, PathBuf};

use cs_app::mission_start::{
    AIRFRAME_TABLE, AIRFRAME_UNKNOWN_REASON, PLAYER_PLANE_KEY, POSE_UNKNOWN_REASON,
    STORED_POSITION_METRES_PER_UNIT, StoredStartPose, airframe_entry, airframe_index,
    recover_retail_start_configuration, scenario_player_airframe,
};
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_field};
use cs_types::content::Resolved;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn list(values: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(values)
}

/// The display names of the table, in the executable's own index order.
const DISPLAY_NAMES: [&str; 11] = [
    "Autogyro",
    "Hellhound",
    "Balmoral",
    "Bloodhawk",
    "Brigand",
    "Devastator",
    "Firebrand",
    "Fury",
    "Kestrel",
    "Peacemaker",
    "Warhawk",
];

#[test]
fn accept_m01_lc_player_airframe_source_table_is_pinned_in_index_order() {
    assert_eq!(AIRFRAME_TABLE.len(), 11);
    let names: Vec<&str> = AIRFRAME_TABLE
        .iter()
        .map(|entry| entry.display_name)
        .collect();
    assert_eq!(names, DISPLAY_NAMES, "the executable's own row order");

    // `airframe_index` answers with `position`, i.e. the *first* row that
    // matches, while the original's `0x426d80` answers `11` (none) for a name
    // two rows match. The two only agree while the display names are distinct,
    // so the invariant the lookup's semantics rest on is pinned here.
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &AIRFRAME_TABLE {
        assert!(
            seen.insert(entry.display_name.to_ascii_lowercase()),
            "the table's display names must be distinct: {:?}",
            entry.display_name
        );
    }

    // Row 3's display name and scene root differ (`bloodhawk` / `player_bhawk`),
    // and row 5's do too (`Devastator` / `player_pfighter` → `piratefighter`):
    // a table keyed on one name alone would be a guess.
    assert_eq!(
        AIRFRAME_TABLE[3],
        cs_app::mission_start::AirframeEntry {
            display_name: "Bloodhawk",
            scene_root: "player_bhawk",
            model: "bloodhawk",
        }
    );
    assert_eq!(
        (AIRFRAME_TABLE[5].scene_root, AIRFRAME_TABLE[5].model),
        ("player_pfighter", "piratefighter")
    );

    // Every scene root is one of the eleven `support\planes.gw` creates, and
    // the model name is the row's own: two rows break any "the model is the
    // display name lowercased" shortcut, which is why both fields are pinned.
    for entry in &AIRFRAME_TABLE {
        assert!(entry.scene_root.starts_with("player_"));
        assert!(!entry.model.is_empty());
    }
    assert_eq!(AIRFRAME_TABLE[1].model, "avenger");
    assert_eq!(AIRFRAME_TABLE[3].model, "bloodhawk");
    assert_eq!(AIRFRAME_TABLE[5].model, "piratefighter");
}

#[test]
fn accept_m01_lc_player_airframe_source_lookup_answers_the_index_the_original_does() {
    // Case-insensitive whole-name comparison, like `0x426d80`'s tolower loop.
    assert_eq!(airframe_index("fury"), Some(7));
    assert_eq!(airframe_index("FURY"), Some(7));
    assert_eq!(airframe_index("Bloodhawk"), Some(3));
    assert_eq!(airframe_index("Devastator"), Some(5));
    assert_eq!(airframe_index("Autogyro"), Some(0));

    // No match is the routine's `11`, which every caller treats as none: an
    // unknown name is never turned into a row.
    assert_eq!(airframe_index("black_widow"), None);
    assert_eq!(airframe_index(""), None);
    assert_eq!(airframe_index("bloodhawk_paint"), None);

    let (index, entry) = airframe_entry("bloodhawk").expect("a measured row");
    assert_eq!(index, 3);
    assert_eq!(entry.scene_root, "player_bhawk");
    assert!(airframe_entry("no_such_airframe").is_none());
}

#[test]
fn accept_m01_lc_player_airframe_source_reads_the_scenario_key_in_both_shapes() {
    let document = list(vec![
        text("mission_type"),
        list(vec![text("dogfight_ace")]),
        text(PLAYER_PLANE_KEY),
        list(vec![text("Fury")]),
        text("num_wingmen"),
        list(vec![ZrdValue::Int(2)]),
    ]);
    let assigned = scenario_player_airframe(&document).expect("the key is present");
    let (index, entry) = airframe_entry(assigned).expect("Fury is a table row");
    assert_eq!((index, entry.scene_root), (7, "player_fury"));

    // The bare spelling the retail `ia.zrd` uses.
    let bare = list(vec![text(PLAYER_PLANE_KEY), text("Bloodhawk")]);
    assert_eq!(scenario_player_airframe(&bare), Some("Bloodhawk"));
    assert_eq!(
        airframe_entry(scenario_player_airframe(&bare).expect("present"))
            .expect("a row")
            .1
            .scene_root,
        "player_bhawk"
    );

    // The pair shape `zrd_field` also reads.
    let pairs = list(vec![list(vec![
        text(PLAYER_PLANE_KEY),
        list(vec![text("Warhawk")]),
    ])]);
    assert_eq!(scenario_player_airframe(&pairs), Some("Warhawk"));

    // Absence answers `None` — a document without the key names no airframe,
    // which is what every campaign mission measured does.
    let other = list(vec![text("mission_type"), list(vec![text("stunt_flying")])]);
    assert_eq!(scenario_player_airframe(&other), None);
    assert_eq!(scenario_player_airframe(&list(vec![])), None);

    // A value that is not one name is refused rather than read as one.
    let two_names = list(vec![
        text(PLAYER_PLANE_KEY),
        list(vec![text("Fury"), text("Bloodhawk")]),
    ]);
    assert_eq!(scenario_player_airframe(&two_names), None);
    let number = list(vec![text(PLAYER_PLANE_KEY), ZrdValue::Int(7)]);
    assert_eq!(scenario_player_airframe(&number), None);
}

#[test]
fn accept_m01_lc_player_airframe_source_stored_position_is_metres_and_the_refusals_name_what_is_left()
 {
    assert_eq!(STORED_POSITION_METRES_PER_UNIT, 1.0);

    // M01's measured player start (#676): the scale change is the whole
    // binding, and the heading it cannot convert stays outside this type.
    let stored = StoredStartPose {
        position: [-3694.0, 1318.0, -12482.0],
        heading: 170.0,
    };
    assert_eq!(stored.position_metres(), [-3694.0, 1318.0, -12482.0]);
    assert_eq!(
        StoredStartPose {
            position: [10.0, 20.0, 30.0],
            heading: 0.0,
        }
        .position_metres(),
        [10.0, 20.0, 30.0]
    );

    // Both refusals record what was measured, so a reader can tell an absent
    // source from an unexamined one.
    for needle in [
        "player_plane",
        "ia.zrd",
        "eleven-row airframe table",
        "no profile or hangar file",
        "F13-B/C, F38",
    ] {
        assert!(
            AIRFRAME_UNKNOWN_REASON.contains(needle),
            "the airframe refusal must record {needle:?}"
        );
    }
    for needle in ["#436", "metre", "handedness", "zero direction"] {
        assert!(
            POSE_UNKNOWN_REASON.contains(needle),
            "the pose refusal must record {needle:?}"
        );
    }
}

/// Reads one member of a reader archive through the production discovery and
/// decoder, the path `recover_retail_start_configuration` takes for `aiv.zrd`.
fn member_document(root: &Path, container_key: &str, member: &str) -> Option<ZrdValue> {
    decoded_members(root, container_key)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(member))
        .map(|(_, document)| document)
}

/// Every member-named program of a reader archive, decoded as `.zrd`.
fn decoded_members(root: &Path, container_key: &str) -> Vec<(String, ZrdValue)> {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery = cs_formats::script_raw::discover_container(container_key, &path, &bytes);
    discovery
        .programs()
        .iter()
        .filter_map(|program| {
            let name = program.locator().member()?.to_owned();
            let document = decode_zrd(program.bytes())
                .unwrap_or_else(|error| panic!("{container_key} {name} decodes: {error:?}"));
            Some((name, document))
        })
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_player_airframe_source_retail_m01_has_no_airframe_key_and_the_scenario_does() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));

    // Measured absence: M01's reader archive names no airframe through the key
    // the executable reads, whatever the member.
    let members = decoded_members(&root, "zbd/c1c/m01/zrdr.zbd");
    assert_eq!(
        members.len(),
        12,
        "M01's reader archive and its members: {:?}",
        members.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    assert!(
        members
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("aiv.zrd")),
        "the aircraft table is one of them"
    );
    for (name, document) in &members {
        assert!(
            zrd_field(document, PLAYER_PLANE_KEY).is_none(),
            "{name} of M01 must not carry {PLAYER_PLANE_KEY}"
        );
        assert!(
            scenario_player_airframe(document).is_none(),
            "{name} of M01 names no player airframe"
        );
    }

    // Measured presence: the instant-action scenario of the same chapter does
    // name one, through that key, and the name is a table row.
    let scenario = member_document(&root, "zbd/c1c/ia1/zrdr.zbd", "ia.zrd").expect("ia.zrd reads");
    let assigned = scenario_player_airframe(&scenario).expect("the scenario names a plane");
    assert_eq!(assigned, "Fury");
    let (index, entry) = airframe_entry(assigned).expect("Fury is a table row");
    assert_eq!(
        (index, entry.scene_root, entry.model),
        (7, "player_fury", "fury")
    );

    // The start configuration itself: metres measured, airframe and heading
    // still refused by name.
    let config = recover_retail_start_configuration(&root, "zbd/c1c/m01").expect("M01 reads");
    let Resolved::Known(stored) = config.stored_pose() else {
        panic!("M01's player record has the pose shape");
    };
    assert_eq!(stored.value.position_metres(), [-3694.0, 1318.0, -12482.0]);
    assert_eq!(stored.value.heading, 170.0);

    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!("no source in M01 assigns an airframe");
    };
    assert_eq!(reason, AIRFRAME_UNKNOWN_REASON);
    let Resolved::Unknown { reason, .. } = config.initial_pose() else {
        panic!("the heading's zero direction is still unmeasured");
    };
    assert_eq!(reason, POSE_UNKNOWN_REASON);
}
