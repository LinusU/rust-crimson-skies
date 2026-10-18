//! Acceptance scenario F55-CU2: the lobby host form's scenario menu.
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md` (follow-up
//! F55-CU2, task #788). The menu is the content-side projection of the F56-A
//! slot catalog: the non-ignored tests read synthetic slot archives through the
//! production `discover_slots` and then call the production
//! `cs_content::lobby::scenario_menu`; removing the projection makes them fail.
//! The `retail` test (`#[ignore = "requires CS_GAME_DIR"]`) builds the menu over
//! the owner's real installation and pins that every slot becomes a selectable
//! option; run it with `--include-ignored`. Without `CS_GAME_DIR` it fails
//! loudly rather than passes.

use std::path::PathBuf;

use cs_content::lobby::scenario_menu;
use cs_content::multiplayer::{ScenarioMode, discover_slots};
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ContentHash;
use cs_types::install::{FileRole, InstallFileRecord, ParseState, RelativePath};

fn hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

fn record(path: &str, size: u64, byte: u8) -> InstallFileRecord {
    InstallFileRecord {
        relative_spelling: RelativePath::new(path).expect("a valid path"),
        size_bytes: size,
        sha256: hash(byte),
        family: None,
        role: FileRole::Unknown,
        parse_state: ParseState::Unparsed,
    }
}

// ---------------------------------------------------------- .zrd / archive ---
//
// The synthetic slots author a real version-one reader archive whose
// `targets.zrd` is written with the measured `.zrd` grammar, so the production
// `discover_slots` locates and decodes the member the same way the installation
// walk does (the writer is the same shape F56-A's acceptance test uses).

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, the child count plus one, then the children.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// One authored `targets.zrd` objective in the measured pair shape.
fn target_record(description: &str) -> Vec<u8> {
    zrd_list(vec![zrd_list(vec![
        zrd_text("description"),
        zrd_text(description),
    ])])
}

/// An authored `targets.zrd` root: one child per objective.
fn targets_document(records: Vec<Vec<u8>>) -> Vec<u8> {
    zrd_list(records)
}

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each, then the u32 version `1` and count.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// One synthetic installation with three readable slots and one gap.
fn fixture_catalog() -> cs_content::multiplayer::SlotCatalog {
    let deathmatch = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        (
            "targets.zrd",
            targets_document(vec![target_record("MSG_TRGT_REARM_BASE")]),
        ),
    ]);
    let flag = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        (
            "targets.zrd",
            targets_document(vec![
                target_record("MSG_TRGT_FLAGBASE"),
                target_record("MSG_TRGT_FLAG"),
            ]),
        ),
    ]);
    // A slot whose archive carries no `targets.zrd`: the mode is an explicit
    // unknown, never a family.
    let no_targets = reader_archive(&[("net.zrd", zrd_int(1))]);
    let files = vec![
        record("ZBD/C1/MP1/zrdr.zbd", 10, 1),
        record("ZBD/C1/MP2/zrdr.zbd", 10, 2),
        record("ZBD/C1B/MP1/zrdr.zbd", 10, 3),
        // A directory with no reader archive: a measured gap, not a scenario.
        record("ZBD/C2B/MP4/mis_anim.zbd", 5, 4),
    ];
    let install = hash(1);
    discover_slots(install, &files, |record| {
        Ok(match record.relative_spelling.as_str() {
            "ZBD/C1/MP1/zrdr.zbd" => deathmatch.clone(),
            "ZBD/C1/MP2/zrdr.zbd" => flag.clone(),
            _ => no_targets.clone(),
        })
    })
    .expect("the slots read")
}

#[test]
fn accept_f55_cu2_each_slot_becomes_a_row_with_its_id_install_spelling_and_mode() {
    let catalog = fixture_catalog();
    let menu = scenario_menu(&catalog);

    // One row per readable slot, in catalog order, each carrying the slot's own
    // content id — never a row number.
    let ids: Vec<String> = menu
        .options
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .collect();
    let slot_ids: Vec<String> = catalog
        .slots
        .iter()
        .map(|slot| slot.id.as_str().to_owned())
        .collect();
    assert_eq!(ids, slot_ids);
    assert_eq!(
        ids,
        [
            "multiplayer_scenario/slot.c1.mp1",
            "multiplayer_scenario/slot.c1.mp2",
            "multiplayer_scenario/slot.c1b.mp1",
        ]
    );
    for row in &menu.options {
        assert_eq!(
            row.id.kind(),
            ContentKind::MultiplayerScenario,
            "the dropdown selects a multiplayer scenario content id"
        );
    }

    // The display label is the installation's own spelling of the directory.
    let labels: Vec<&str> = menu.options.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(labels, ["ZBD/C1/MP1", "ZBD/C1/MP2", "ZBD/C1B/MP1"]);

    // The mode family is the slot's own, and the unreadable slot stays an
    // explicit unknown rather than being defaulted to a family.
    assert_eq!(
        menu.options[0].mode.clone().known(),
        Some(ScenarioMode::Deathmatch)
    );
    assert_eq!(
        menu.options[1].mode.clone().known(),
        Some(ScenarioMode::CaptureTheFlag)
    );
    let Resolved::Unknown { reason, .. } = &menu.options[2].mode else {
        panic!("a slot with no targets member must stay unknown");
    };
    assert!(!reason.is_empty());

    // A directory the installation ships without a reader archive is listed as
    // unavailable and never invented into a selectable row.
    assert_eq!(menu.unavailable, ["ZBD/C2B/MP4"]);
    assert!(
        menu.options.iter().all(|row| !row.label.contains("C2B")),
        "an unavailable directory is not a scenario"
    );

    // Re-resolving a stored selection uses the id, and an id the installation
    // does not offer is `None`, never the first row.
    assert_eq!(menu.get(&menu.options[1].id), Some(&menu.options[1]));
    let absent =
        ContentId::from_source(ContentKind::MultiplayerScenario, "slot.zz.mp9").expect("an id");
    assert_eq!(menu.get(&absent), None);
    let wrong_kind =
        ContentId::from_source(ContentKind::MultiplayerRules, "slot.c1.mp1").expect("an id");
    assert_eq!(menu.get(&wrong_kind), None);
}

#[test]
fn accept_f55_cu2_an_installation_with_no_slot_offers_an_empty_menu() {
    let catalog = discover_slots(hash(1), &[], |_| Ok(Vec::new())).expect("an empty walk reads");
    let menu = scenario_menu(&catalog);
    assert!(menu.is_empty());
    assert!(menu.options.is_empty());
    assert!(menu.unavailable.is_empty());
}

// --------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

fn retail_menu() -> cs_content::lobby::ScenarioMenu {
    let dir = game_dir();
    let found = cs_assets::install::discover(&dir).expect("discovery reads the installation");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let catalog = discover_slots(install, &found.manifest.files, |record| {
        std::fs::read(dir.join(record.relative_spelling.as_str()))
    })
    .expect("every slot archive reads");
    scenario_menu(&catalog)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f55_cu2_retail_every_shipped_slot_is_a_selectable_row() {
    let menu = retail_menu();
    assert!(!menu.is_empty(), "the installation ships scenarios to host");
    assert!(menu.unavailable.is_empty(), "{:?}", menu.unavailable);

    // Every row is a distinct multiplayer scenario id with the installation's
    // own spelling, and re-resolving it by id returns the same row.
    let mut ids: Vec<&str> = menu.options.iter().map(|row| row.id.as_str()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "scenario ids are unique");
    let mut labels: Vec<&str> = menu.options.iter().map(|row| row.label.as_str()).collect();
    let before = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), before, "install spellings are unique");
    for row in &menu.options {
        assert_eq!(row.id.kind(), ContentKind::MultiplayerScenario);
        assert_eq!(menu.get(&row.id), Some(row));
    }
}
