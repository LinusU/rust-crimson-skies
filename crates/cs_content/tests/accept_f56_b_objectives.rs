//! Acceptance scenario F56-B (content half): a scenario slot exposes its
//! decoded `targets.zrd` objective records, classified by the measured
//! description keys, with explicit unknowns where the member is missing.
//!
//! The synthetic tests author real version-one reader archives through the
//! production `discover_slots`; the retail test reads the owner's
//! installation (`#[ignore = "requires CS_GAME_DIR"]`).

use std::path::PathBuf;

use cs_content::multiplayer::{ObjectiveKind, ScenarioMode, SlotObjective, discover_slots};
use cs_types::content::Resolved;
use cs_types::evidence::{ClaimStatus, ContentHash};
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

// -------------------------------------------------- .zrd / archive authors ---
//
// The same measured `.zrd` grammar as `accept_f56_a_catalog.rs`, written
// independently of the decoder: tag 1 int, tag 3 text, tag 4 list whose count
// is `children + 1`.

fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A `key, value` field pair, or `key` alone when `value` is `None` (the
/// measured bare directives like `objective`).
fn field(key: &str, value: Option<Vec<u8>>) -> Vec<u8> {
    let mut children = vec![zrd_text(key)];
    if let Some(value) = value {
        children.push(value);
    }
    zrd_list(children)
}

/// A version-one reader archive holding `members` in order: member data, then
/// one 148-byte index entry each, then the version and count.
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
        let mut name_field = [0_u8; 64];
        name_field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&name_field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

fn objectives_of(catalog: &cs_content::multiplayer::SlotCatalog, index: usize) -> &[SlotObjective] {
    let Resolved::Known(known) = &catalog.slots[index].objectives else {
        panic!("slot {index} must resolve its objective records");
    };
    known.value.as_slice()
}

#[test]
fn accept_f56_b_objective_records_decode_with_their_measured_fields() {
    let flag = zrd_list(vec![
        field("description", Some(zrd_text("MSG_TRGT_FLAG"))),
        field("nodes", Some(zrd_list(vec![zrd_text("flag_node")]))),
        field("help_label", Some(zrd_text("MSG_OBJ_TEAM_1"))),
        field("objective", None),
    ]);
    let base = zrd_list(vec![
        field("description", Some(zrd_text("MSG_TRGT_FLAGBASE"))),
        field("nodes", Some(zrd_list(vec![zrd_text("base_node")]))),
        field("other_target", None),
    ]);
    let archive = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        ("targets.zrd", zrd_list(vec![flag, base])),
    ]);
    let files = vec![record("ZBD/C1/MP1/zrdr.zbd", 10, 1)];
    let catalog = discover_slots(hash(1), &files, |_| Ok(archive.clone())).expect("the slot reads");

    let objectives = objectives_of(&catalog, 0);
    assert_eq!(objectives.len(), 2);
    assert_eq!(objectives[0].index, 0);
    assert_eq!(objectives[0].description.as_deref(), Some("MSG_TRGT_FLAG"));
    assert_eq!(objectives[0].nodes, ["flag_node"]);
    assert_eq!(objectives[0].help_label.as_deref(), Some("MSG_OBJ_TEAM_1"));
    assert_eq!(objectives[0].directives, ["objective"]);
    assert_eq!(objectives[0].kind(), ObjectiveKind::Flag);
    assert!(objectives[0].possessable());
    assert_eq!(objectives[1].kind(), ObjectiveKind::FlagBase);
    assert!(!objectives[1].possessable());
    assert_eq!(objectives[1].directives, ["other_target"]);

    // The records carry the member's own span and the observed-tool class.
    let Resolved::Known(known) = &catalog.slots[0].objectives else {
        panic!("resolved above");
    };
    assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
    let span = known.provenance.source.as_ref().expect("a source span");
    assert_eq!(span.member_key(), Some("targets.zrd"));
    // And the decoded list still drives the mode binding.
    assert_eq!(
        catalog.slots[0].mode.clone().known(),
        Some(ScenarioMode::CaptureTheFlag)
    );
}

#[test]
fn accept_f56_b_the_measured_description_keys_classify_and_only_flags_hold() {
    let kind = |description: &str| {
        SlotObjective {
            index: 0,
            description: Some(description.to_owned()),
            nodes: Vec::new(),
            help_label: None,
            category_label: None,
            directives: Vec::new(),
        }
        .kind()
    };
    assert_eq!(kind("MSG_TRGT_FLAGBASE"), ObjectiveKind::FlagBase);
    assert_eq!(kind("MSG_TRGT_FLAG"), ObjectiveKind::Flag);
    assert_eq!(kind("MSG_TRGT_ZEP_ENEMY"), ObjectiveKind::ZeppelinEnemy);
    assert_eq!(kind("MSG_TRGT_ZEP_FRIEND"), ObjectiveKind::ZeppelinFriend);
    assert_eq!(kind("MSG_TRGT_REARM_BASE"), ObjectiveKind::RearmBase);
    assert_eq!(kind("MSG_TRGT_SOMETHING_ELSE"), ObjectiveKind::Other);

    let no_description = SlotObjective {
        index: 0,
        description: None,
        nodes: Vec::new(),
        help_label: None,
        category_label: None,
        directives: Vec::new(),
    };
    assert_eq!(no_description.kind(), ObjectiveKind::Other);
    assert!(!no_description.possessable());
}

#[test]
fn accept_f56_b_a_slot_without_a_usable_targets_member_reports_unknown_records() {
    let no_member = reader_archive(&[("net.zrd", zrd_int(1))]);
    let undecodable = reader_archive(&[("targets.zrd", b"not a .zrd".to_vec())]);
    let not_a_list = reader_archive(&[("targets.zrd", zrd_text("MSG_TRGT_FLAG"))]);
    let files = vec![
        record("ZBD/C1/MP1/zrdr.zbd", 10, 1),
        record("ZBD/C1/MP2/zrdr.zbd", 10, 2),
        record("ZBD/C1/MP3/zrdr.zbd", 10, 3),
    ];
    let catalog = discover_slots(hash(1), &files, |record| {
        Ok(match record.relative_spelling.as_str() {
            "ZBD/C1/MP1/zrdr.zbd" => no_member.clone(),
            "ZBD/C1/MP2/zrdr.zbd" => undecodable.clone(),
            _ => not_a_list.clone(),
        })
    })
    .expect("the slots read");
    for slot in &catalog.slots {
        let Resolved::Unknown { reason, .. } = &slot.objectives else {
            panic!("{} must stay unknown", slot.id);
        };
        assert!(!reason.is_empty(), "{}", slot.id);
    }
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f56_b_retail_every_slot_exposes_decoded_objective_records() {
    use std::collections::BTreeSet;

    let dir = game_dir();
    let found = cs_assets::install::discover(&dir).expect("discovery reads the installation");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let catalog = discover_slots(install, &found.manifest.files, |record| {
        std::fs::read(dir.join(record.relative_spelling.as_str()))
    })
    .expect("every slot archive reads");
    assert_eq!(catalog.slots.len(), 21);

    // The measured vocabulary of the whole installation's records: five
    // descriptions, two bare directives, the zeppelin category, four help
    // labels. Anything else must fail here, never be classified silently.
    let mut descriptions = BTreeSet::new();
    let mut directives = BTreeSet::new();
    let mut categories = BTreeSet::new();
    let mut helps = BTreeSet::new();
    let mut possessable_slots = BTreeSet::new();
    let mut zeppelin_slots = BTreeSet::new();
    for slot in &catalog.slots {
        let name = format!("{}/MP{}", slot.world_group, slot.slot);
        let Resolved::Known(known) = &slot.objectives else {
            panic!(
                "{name} must resolve its objective records: {:?}",
                slot.objectives
            );
        };
        assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        let span = known.provenance.source.as_ref().expect("a source span");
        assert_eq!(span.container_path(), slot.program.container_path());
        assert_eq!(span.member_key(), Some("targets.zrd"));
        assert!(slot.objectives.provenance().is_some());
        for (index, objective) in known.value.iter().enumerate() {
            assert_eq!(objective.index as usize, index, "{name} keeps list order");
            if let Some(description) = &objective.description {
                descriptions.insert(description.clone());
            }
            for directive in &objective.directives {
                directives.insert(directive.clone());
            }
            if let Some(category) = &objective.category_label {
                categories.insert(category.clone());
            }
            if let Some(help) = &objective.help_label {
                helps.insert(help.clone());
            }
        }
        if known.value.iter().any(SlotObjective::possessable) {
            possessable_slots.insert(name.clone());
        }
        if known
            .value
            .iter()
            .any(|objective| objective.kind() == ObjectiveKind::ZeppelinEnemy)
        {
            zeppelin_slots.insert(name.clone());
        }
    }
    assert_eq!(
        descriptions.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "MSG_TRGT_FLAG",
            "MSG_TRGT_FLAGBASE",
            "MSG_TRGT_REARM_BASE",
            "MSG_TRGT_ZEP_ENEMY",
            "MSG_TRGT_ZEP_FRIEND",
        ]
    );
    assert_eq!(
        directives.iter().map(String::as_str).collect::<Vec<_>>(),
        ["objective", "other_target"]
    );
    assert_eq!(
        categories.iter().map(String::as_str).collect::<Vec<_>>(),
        ["MSG_OBJ_ZEPPELIN"]
    );
    assert_eq!(
        helps.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "MSG_OBJ_DEFEND",
            "MSG_OBJ_DESTROY",
            "MSG_OBJ_TEAM_1",
            "MSG_OBJ_TEAM_2",
        ]
    );

    // Possessable flags occur exactly in the slots the binding calls Capture
    // the Flag, and the enemy zeppelin marks exactly the Zeppelin slots: the
    // classification and the mode binding agree on every slot.
    let mode_slots: BTreeSet<String> = catalog
        .slots
        .iter()
        .filter(|slot| {
            slot.mode
                .clone()
                .known()
                .is_some_and(|mode| mode == ScenarioMode::CaptureTheFlag)
        })
        .map(|slot| format!("{}/MP{}", slot.world_group, slot.slot))
        .collect();
    assert_eq!(possessable_slots, mode_slots);
    assert_eq!(
        mode_slots.len(),
        5,
        "every world group's MP2 is the flag slot"
    );
    let zeppelin_mode_slots: BTreeSet<String> = catalog
        .slots
        .iter()
        .filter(|slot| {
            slot.mode
                .clone()
                .known()
                .is_some_and(|mode| mode == ScenarioMode::ZeppelinVsZeppelin)
        })
        .map(|slot| format!("{}/MP{}", slot.world_group, slot.slot))
        .collect();
    assert_eq!(zeppelin_slots, zeppelin_mode_slots);
}
