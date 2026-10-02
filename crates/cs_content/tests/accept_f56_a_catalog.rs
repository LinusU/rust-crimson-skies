//! Acceptance scenario F56-A (content half): the original multiplayer mode and
//! scenario-slot catalog (`specs/F56-original-multiplayer-scenarios-and-mode-
//! rules.md`, `### F56-A`).
//!
//! The non-ignored tests read synthetic string rows and a synthetic file list
//! through the production `discover_modes` / `discover_slots`; removing either
//! discovery makes them fail. The `retail` tests
//! (`#[ignore = "requires CS_GAME_DIR"]`) read the owner's installation and
//! pin what it holds; run them with `--include-ignored`. Without `CS_GAME_DIR`
//! they fail loudly rather than pass.

use std::io;
use std::path::PathBuf;

use cs_content::config::StringRow;
use cs_content::multiplayer::{
    BRIEFING_FIRST_ID, BRIEFING_STRIDE, ModeError, RULE_LABELS, ScenarioMode, SlotMarker, TeamPlay,
    discover_modes, discover_slots, scan_markers,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentKind, Resolved};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{FileRole, InstallFileRecord, ParseState, RelativePath};

const LANGUAGE: u32 = 1033;

fn hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

fn span() -> SourceSpan {
    SourceSpan::new(hash(1), "strings.dll", None, 0, 100, None).expect("a valid span")
}

fn row(id: u32, text: &str) -> StringRow {
    StringRow {
        id,
        language: LANGUAGE,
        code_page: 1252,
        code_units: text.encode_utf16().collect(),
        text: Some(text.to_owned()),
        span: span(),
    }
}

/// The four mode names and four briefing blocks, shaped like the retail table
/// (these are authored fixture strings, not original text), followed by a
/// block of another family.
fn fixture_rows() -> Vec<StringRow> {
    let mut rows = vec![
        row(7010, "score"),
        row(7011, "Deathmatch without Teams"),
        row(7012, "Deathmatch with Teams"),
        row(7013, "Capture the Flag"),
        row(7014, "Zeppelin vs. Zeppelin"),
        row(7015, "TCP/IP"),
    ];
    let blocks: [(&str, &[&str], &[&str]); 4] = [
        (
            "CAPTURE THE FLAG",
            &["take it", "bring it back", "your team scores"],
            &["10", "8", "2", "-2"],
        ),
        (
            "ZEPPELIN vs. ZEPPELIN",
            &["sink theirs", "protect your team"],
            &["10", "2", "-2"],
        ),
        ("DEATHMATCH", &["shoot them", "do not crash"], &["2", "-2"]),
        (
            "TEAM DEATHMATCH",
            &["shoot them", "not your team"],
            &["2", "-2"],
        ),
    ];
    for (index, (title, instructions, points)) in blocks.iter().enumerate() {
        let base = BRIEFING_FIRST_ID + index as u32 * BRIEFING_STRIDE;
        rows.push(row(base, title));
        rows.push(row(base + 1, "a tagline"));
        rows.push(row(base + 2, "POINTS"));
        rows.push(row(base + 3, "INSTRUCTIONS"));
        let mut next = base + 4;
        for text in *instructions {
            rows.push(row(next, text));
            next += 1;
        }
        for text in *points {
            rows.push(row(next, text));
            next += 1;
        }
    }
    let other = BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE;
    rows.push(row(other, "INSTANT ACTION"));
    rows.push(row(other + 1, "DOGFIGHT AN ACE."));
    rows.push(row(other + 2, "a different family"));
    // The original blocks pad the unused rows of their fixed stride with empty
    // strings (ids 16614..16619 in the installation). Padding is not an
    // instruction and not a printed point value.
    let first = BRIEFING_FIRST_ID;
    for offset in 11..BRIEFING_STRIDE {
        rows.push(row(first + offset, ""));
    }
    rows
}

#[test]
fn accept_f56_a_every_named_mode_pairs_with_its_briefing_and_its_points() {
    let catalog = discover_modes(&fixture_rows(), LANGUAGE).expect("the fixture table reads");
    assert!(catalog.is_complete(), "{catalog:#?}");
    assert_eq!(catalog.briefing_blocks, 4);
    assert_eq!(
        catalog.boundary.as_ref().map(|row| row.text.as_str()),
        Some("INSTANT ACTION"),
        "the walk stops at the first block of another family and reports it"
    );
    let by_name: Vec<(&str, &str, Vec<i32>)> = catalog
        .modes
        .iter()
        .map(|mode| {
            (
                mode.name.text.as_str(),
                mode.briefing.title.text.as_str(),
                mode.briefing.points.iter().map(|p| p.value).collect(),
            )
        })
        .collect();
    assert_eq!(
        by_name,
        [
            ("Deathmatch without Teams", "DEATHMATCH", vec![2, -2]),
            ("Deathmatch with Teams", "TEAM DEATHMATCH", vec![2, -2]),
            ("Capture the Flag", "CAPTURE THE FLAG", vec![10, 8, 2, -2]),
            (
                "Zeppelin vs. Zeppelin",
                "ZEPPELIN vs. ZEPPELIN",
                vec![10, 2, -2]
            ),
        ]
    );
    // A number row is a point label, never an instruction line.
    assert!(
        catalog.modes[2]
            .briefing
            .instructions
            .iter()
            .all(|line| line.text.parse::<i32>().is_err())
    );
    assert_eq!(catalog.modes[2].briefing.instructions.len(), 3);
}

#[test]
fn accept_f56_a_unstated_rules_stay_unknown_and_team_play_keeps_its_evidence_class() {
    let catalog = discover_modes(&fixture_rows(), LANGUAGE).expect("reads");
    let teams: Vec<Option<TeamPlay>> = catalog
        .modes
        .iter()
        .map(|mode| match &mode.team_play {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        })
        .collect();
    assert_eq!(
        teams,
        [
            Some(TeamPlay::FreeForAll),
            Some(TeamPlay::Teams),
            Some(TeamPlay::Teams),
            Some(TeamPlay::Teams),
        ]
    );
    // The name states team play; the Capture the Flag briefing only implies it.
    let class = |index: usize| match &catalog.modes[index].team_play {
        Resolved::Known(known) => known.provenance.class.label(),
        Resolved::Unknown { .. } => "unknown",
    };
    assert_eq!(class(0), "observed_tool");
    assert_eq!(class(2), "inferred");
    for mode in &catalog.modes {
        let labels: Vec<&str> = mode.unknown_rules.iter().map(|rule| rule.field).collect();
        let expected: Vec<&str> = RULE_LABELS.into_iter().filter(|l| *l != "teams").collect();
        assert_eq!(labels, expected, "{}", mode.name.text);
        assert!(
            mode.unknown_rules
                .iter()
                .all(|rule| !rule.reason.is_empty())
        );
        assert_eq!(mode.id.kind(), ContentKind::MultiplayerRules);
    }
}

#[test]
fn accept_f56_a_a_briefing_without_a_name_is_a_gap_and_is_not_dropped() {
    // A fifth block of the family that no mode name answers.
    let extra = BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE;
    let mut rows = fixture_rows();
    rows.retain(|row| !(extra..extra + BRIEFING_STRIDE).contains(&row.id));
    rows.push(row(extra, "KING OF THE HILL"));
    rows.push(row(extra + 1, "a tagline"));
    rows.push(row(extra + 2, "POINTS"));
    rows.push(row(extra + 3, "INSTRUCTIONS"));
    rows.push(row(extra + 4, "hold the hill"));
    rows.push(row(extra + 5, "5"));
    let boundary = extra + BRIEFING_STRIDE;
    rows.push(row(boundary, "INSTANT ACTION"));
    rows.push(row(boundary + 2, "a different family"));
    let catalog = discover_modes(&rows, LANGUAGE).expect("reads");
    assert!(!catalog.is_complete());
    assert_eq!(catalog.modes.len(), 4);
    assert_eq!(catalog.briefing_blocks, 5);
    assert_eq!(catalog.briefings_without_name.len(), 1);
    assert_eq!(
        catalog.briefings_without_name[0].title.text,
        "KING OF THE HILL"
    );
    assert_eq!(catalog.briefings_without_name[0].points[0].value, 5);
}

#[test]
fn accept_f56_a_a_name_without_a_briefing_is_a_gap_and_is_not_dropped() {
    let mut rows = fixture_rows();
    let base = BRIEFING_FIRST_ID + 3 * BRIEFING_STRIDE;
    rows.retain(|row| !(base..base + BRIEFING_STRIDE).contains(&row.id));
    let catalog = discover_modes(&rows, LANGUAGE).expect("reads");
    assert!(!catalog.is_complete());
    assert_eq!(catalog.names_without_briefing.len(), 1);
    assert_eq!(
        catalog.names_without_briefing[0].text,
        "Deathmatch with Teams"
    );
}

#[test]
fn accept_f56_a_a_missing_ambiguous_or_undecodable_row_is_an_error() {
    let mut missing = fixture_rows();
    missing.retain(|row| row.id != 7013);
    assert!(matches!(
        discover_modes(&missing, LANGUAGE),
        Err(ModeError::MissingRow { id: 7013 })
    ));

    let mut duplicate = fixture_rows();
    duplicate.push(row(7011, "Deathmatch without Teams"));
    assert!(matches!(
        discover_modes(&duplicate, LANGUAGE),
        Err(ModeError::Duplicate { id: 7011 })
    ));

    let mut undecodable = fixture_rows();
    undecodable
        .iter_mut()
        .find(|row| row.id == 7014)
        .unwrap()
        .text = None;
    assert!(matches!(
        discover_modes(&undecodable, LANGUAGE),
        Err(ModeError::Undecodable { id: 7014 })
    ));

    let no_blocks: Vec<StringRow> = fixture_rows()
        .into_iter()
        .filter(|r| r.id < 16000)
        .collect();
    assert!(matches!(
        discover_modes(&no_blocks, LANGUAGE),
        Err(ModeError::NoBriefing)
    ));
}

#[test]
fn accept_f56_a_rows_of_another_language_are_not_mixed_in() {
    let mut rows = fixture_rows();
    for row in &mut rows {
        row.language = 1031;
    }
    assert!(matches!(
        discover_modes(&rows, LANGUAGE),
        Err(ModeError::MissingRow { id: 7011 })
    ));
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

#[test]
fn accept_f56_a_slots_are_found_per_world_group_with_markers_and_an_unknown_mode() {
    let files = vec![
        record("ZBD/C1/MP1/zrdr.zbd", 10, 11),
        record("ZBD/C1/MP1/mis_anim.zbd", 5, 12),
        record("ZBD/C1/MP2/zrdr.zbd", 20, 13),
        record("ZBD/C1B/MP3/zrdr.zbd", 30, 14),
        record("ZBD/C1B/MP4/mis_anim.zbd", 5, 15),
        // Not slots: a mission, an IA directory, a deeper path, a bad number.
        record("ZBD/C1/M02/zrdr.zbd", 40, 16),
        record("ZBD/C1/IA1/zrdr.zbd", 40, 17),
        record("ZBD/C1/MP1/sub/zrdr.zbd", 40, 18),
        record("ZBD/C1/MP01/zrdr.zbd", 40, 19),
        record("ZBD/C1/MP0/zrdr.zbd", 40, 20),
    ];
    let bytes = |path: &str| -> Vec<u8> {
        match path {
            "ZBD/C1/MP1/zrdr.zbd" => b"Player Airport".to_vec(),
            "ZBD/C1/MP2/zrdr.zbd" => b"xx FLAG BASE 1 xx".to_vec(),
            _ => b"..\\data\\common\\ZRDR\\Zeps\\a.zrd".to_vec(),
        }
    };
    let catalog = discover_slots(hash(1), &files, |record| {
        Ok(bytes(record.relative_spelling.as_str()))
    })
    .expect("the slots read");

    let summary: Vec<(String, u8, Vec<SlotMarker>)> = catalog
        .slots
        .iter()
        .map(|slot| (slot.world_group.clone(), slot.slot, slot.markers.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("C1".to_owned(), 1, vec![]),
            ("C1".to_owned(), 2, vec![SlotMarker::FlagBase]),
            ("C1B".to_owned(), 3, vec![SlotMarker::ZeppelinData]),
        ]
    );
    // A gap is spelled as the install manifest spells the directory, so it
    // joins with `ScenarioSlot::world_group` and the manifest alike.
    assert_eq!(catalog.without_program, ["ZBD/C1B/MP4"]);
    assert_eq!(catalog.slots[0].companions, ["ZBD/C1/MP1/mis_anim.zbd"]);
    assert_eq!(
        catalog.slots[0].id.as_str(),
        "multiplayer_scenario/slot.c1.mp1"
    );
    assert_eq!(catalog.slots[0].program.length(), 10);
    assert_eq!(catalog.slots[0].program.member_sha256(), Some(hash(11)));
    for slot in &catalog.slots {
        assert!(
            matches!(slot.mode, Resolved::Unknown { .. }),
            "a slot's mode is not known from its markers"
        );
    }
}

#[test]
fn accept_f56_a_an_unreadable_slot_archive_is_an_error_not_a_skipped_slot() {
    let files = vec![record("ZBD/C1/MP1/zrdr.zbd", 10, 11)];
    let result = discover_slots(hash(1), &files, |_| Err(io::Error::other("denied")));
    assert!(result.is_err());
}

// -------------------------------------------------- .zrd / archive authors ---
//
// The synthetic slot tests author a real version-one reader archive whose
// `targets.zrd` is written with the measured `.zrd` grammar, so the production
// `discover_slots` locates and decodes the member through the same container
// discovery the installation walk uses. The writer is independent of the
// decoder: the decoder's own accepted shape is what the test proves.

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

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count, then
/// the children (the measured grammar: the profiler stores one more than the
/// child count).
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
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
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

#[test]
fn accept_f56_a_a_slot_binds_to_the_mode_its_targets_member_names() {
    // `net.zrd` first so the `targets.zrd` member starts at a non-zero offset
    // the span must record.
    let deathmatch = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        (
            "targets.zrd",
            targets_document(vec![target_record("MSG_TRGT_REARM_BASE")]),
        ),
    ]);
    let empty = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        ("targets.zrd", targets_document(vec![])),
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
    let zeppelin = reader_archive(&[
        ("net.zrd", zrd_int(1)),
        (
            "targets.zrd",
            targets_document(vec![
                target_record("MSG_TRGT_ZEP_ENEMY"),
                target_record("MSG_TRGT_ZEP_FRIEND"),
            ]),
        ),
    ]);
    let files = vec![
        record("ZBD/C1/MP1/zrdr.zbd", 10, 1),
        record("ZBD/C1/MP2/zrdr.zbd", 10, 2),
        record("ZBD/C1/MP3/zrdr.zbd", 10, 3),
        record("ZBD/C1B/MP1/zrdr.zbd", 10, 4),
    ];
    let catalog = discover_slots(hash(1), &files, |record| {
        Ok(match record.relative_spelling.as_str() {
            "ZBD/C1/MP1/zrdr.zbd" => deathmatch.clone(),
            "ZBD/C1/MP2/zrdr.zbd" => flag.clone(),
            "ZBD/C1/MP3/zrdr.zbd" => zeppelin.clone(),
            _ => empty.clone(),
        })
    })
    .expect("the slots read");

    let modes: Vec<(String, ScenarioMode)> = catalog
        .slots
        .iter()
        .map(|slot| {
            (
                format!("{}/MP{}", slot.world_group, slot.slot),
                slot.mode
                    .clone()
                    .known()
                    .expect("the targets member names a mode"),
            )
        })
        .collect();
    assert_eq!(
        modes,
        [
            ("C1/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C1/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C1/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            // A `targets.zrd` with no objective names neither a flag nor a
            // zeppelin, so it is deathmatch too.
            ("C1B/MP1".to_owned(), ScenarioMode::Deathmatch),
        ]
    );

    // The binding carries the member's own span and the observed-tool class.
    for slot in &catalog.slots {
        let Resolved::Known(known) = &slot.mode else {
            panic!("{} is unresolved", slot.id);
        };
        assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        let span = known.provenance.source.as_ref().expect("a source span");
        assert_eq!(span.container_path(), slot.program.container_path());
        assert_eq!(span.member_key(), Some("targets.zrd"));
        assert!(span.length() > 0);
        assert!(span.member_sha256().is_some());
    }
    // `net.zrd` is one 8-byte int node, so the first `targets.zrd` starts at 8.
    let span = catalog.slots[0]
        .mode
        .provenance()
        .and_then(|provenance| provenance.source.as_ref())
        .expect("a span");
    assert_eq!(span.offset(), 8);
    assert_eq!(
        span.member_sha256(),
        Some(cs_assets::install::sha256(&targets_document(vec![
            target_record("MSG_TRGT_REARM_BASE")
        ])))
    );
}

#[test]
fn accept_f56_a_a_slot_without_a_usable_targets_member_stays_unknown() {
    let no_member = reader_archive(&[("net.zrd", zrd_int(1))]);
    let undecodable = reader_archive(&[("targets.zrd", b"not a .zrd".to_vec())]);
    let not_a_list = reader_archive(&[("targets.zrd", zrd_text("MSG_TRGT_FLAG"))]);
    let contradictory = reader_archive(&[(
        "targets.zrd",
        targets_document(vec![
            target_record("MSG_TRGT_FLAGBASE"),
            target_record("MSG_TRGT_ZEP_ENEMY"),
        ]),
    )]);
    let files = vec![
        record("ZBD/C1/MP1/zrdr.zbd", 10, 1),
        record("ZBD/C1/MP2/zrdr.zbd", 10, 2),
        record("ZBD/C1/MP3/zrdr.zbd", 10, 3),
        record("ZBD/C1B/MP1/zrdr.zbd", 10, 4),
    ];
    let catalog = discover_slots(hash(1), &files, |record| {
        Ok(match record.relative_spelling.as_str() {
            "ZBD/C1/MP1/zrdr.zbd" => no_member.clone(),
            "ZBD/C1/MP2/zrdr.zbd" => undecodable.clone(),
            "ZBD/C1/MP3/zrdr.zbd" => not_a_list.clone(),
            _ => contradictory.clone(),
        })
    })
    .expect("the slots read");
    for slot in &catalog.slots {
        let Resolved::Unknown { reason, .. } = &slot.mode else {
            panic!("{} must stay unknown", slot.id);
        };
        assert!(!reason.is_empty(), "{}", slot.id);
    }
}

#[test]
fn accept_f56_a_each_scenario_mode_covers_only_named_modes() {
    use cs_content::multiplayer::MODE_NAME_IDS;

    for mode in [
        ScenarioMode::Deathmatch,
        ScenarioMode::CaptureTheFlag,
        ScenarioMode::ZeppelinVsZeppelin,
    ] {
        let ids = mode.mode_name_ids();
        assert!(!ids.is_empty(), "{}", mode.label());
        for id in ids {
            assert!(MODE_NAME_IDS.contains(id), "{} covers {id}", mode.label());
        }
    }
    assert_eq!(ScenarioMode::Deathmatch.mode_name_ids(), [7011, 7012]);
    assert_eq!(ScenarioMode::CaptureTheFlag.mode_name_ids(), [7013]);
    assert_eq!(ScenarioMode::ZeppelinVsZeppelin.mode_name_ids(), [7014]);
}

#[test]
fn accept_f56_a_marker_scan_is_case_insensitive_and_exact() {
    assert_eq!(scan_markers(b"a Flag Base 2"), [SlotMarker::FlagBase]);
    assert_eq!(scan_markers(b"x\\ZEPS\\y"), [SlotMarker::ZeppelinData]);
    assert!(scan_markers(b"flagbase zeps").is_empty());
    assert!(scan_markers(b"").is_empty());
}

#[test]
fn accept_f56_a_rule_labels_mirror_the_network_rule_fields() {
    let net: Vec<&str> = cs_net::rules::RuleField::ALL
        .iter()
        .map(|field| field.label())
        .collect();
    assert_eq!(net, RULE_LABELS);
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

fn retail_modes() -> cs_content::multiplayer::ModeCatalog {
    let dir = game_dir();
    let bytes = std::fs::read(dir.join("strings.dll")).expect("the installation holds strings.dll");
    let found = cs_assets::install::discover(&dir).expect("discovery reads the installation");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let source = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("a valid span");
    let mut context = cs_formats::ParseContext::with_defaults("strings.dll");
    let strings = cs_content::config::StringCatalog::read(&mut context, source, &bytes)
        .expect("the production reader reads strings.dll");
    discover_modes(strings.rows(), LANGUAGE).expect("the mode table reads")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f56_a_retail_the_installation_names_exactly_four_modes_each_with_a_briefing() {
    let catalog = retail_modes();
    assert!(catalog.is_complete(), "{catalog:#?}");
    let names: Vec<&str> = catalog.modes.iter().map(|m| m.name.text.as_str()).collect();
    assert_eq!(
        names,
        [
            "Deathmatch without Teams",
            "Deathmatch with Teams",
            "Capture the Flag",
            "Zeppelin vs. Zeppelin",
        ]
    );
    let titles: Vec<&str> = catalog
        .modes
        .iter()
        .map(|m| m.briefing.title.text.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "DEATHMATCH",
            "TEAM DEATHMATCH",
            "CAPTURE THE FLAG",
            "ZEPPELIN vs. ZEPPELIN"
        ]
    );
    let points: Vec<Vec<i32>> = catalog
        .modes
        .iter()
        .map(|m| m.briefing.points.iter().map(|p| p.value).collect())
        .collect();
    assert_eq!(
        points,
        [
            vec![2, -2],
            vec![2, -2],
            vec![10, 8, 2, -2],
            vec![10, 2, -2]
        ]
    );
    // The instruction count excludes the empty rows the blocks pad their fixed
    // stride with (ids 16614..16619, 16633..16639, 16648..16659, 16668..16679).
    let instructions: Vec<usize> = catalog
        .modes
        .iter()
        .map(|mode| mode.briefing.instructions.len())
        .collect();
    assert_eq!(instructions, [2, 2, 6, 6]);
    for mode in &catalog.modes {
        assert!(
            mode.briefing
                .instructions
                .iter()
                .all(|line| !line.text.trim().is_empty()),
            "{}",
            mode.name.text
        );
    }
    // The table ends where the Instant Action family begins: that boundary is
    // what makes "four" a measurement and not an assumption.
    assert_eq!(catalog.briefing_blocks, 4);
    assert_eq!(
        catalog.boundary.as_ref().map(|row| row.text.as_str()),
        Some("INSTANT ACTION")
    );
    let teams: Vec<Option<TeamPlay>> = catalog
        .modes
        .iter()
        .map(|mode| match &mode.team_play {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        })
        .collect();
    assert_eq!(
        teams,
        [
            Some(TeamPlay::FreeForAll),
            Some(TeamPlay::Teams),
            Some(TeamPlay::Teams),
            Some(TeamPlay::Teams),
        ]
    );
    for mode in &catalog.modes {
        assert!(
            !mode.unknown_rules.is_empty(),
            "no original rule value is claimed"
        );
        assert!(mode.name.span.length() > 0);
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f56_a_retail_every_scenario_slot_binds_to_a_mode_with_evidence() {
    let dir = game_dir();
    let found = cs_assets::install::discover(&dir).expect("discovery reads the installation");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let catalog = discover_slots(install, &found.manifest.files, |record| {
        std::fs::read(dir.join(record.relative_spelling.as_str()))
    })
    .expect("every slot archive reads");
    assert!(
        catalog.without_program.is_empty(),
        "{:?}",
        catalog.without_program
    );

    let slots: Vec<String> = catalog
        .slots
        .iter()
        .map(|slot| format!("{}/MP{}", slot.world_group, slot.slot))
        .collect();
    assert_eq!(
        slots,
        [
            "C1/MP1", "C1/MP2", "C1/MP3", "C1B/MP1", "C1B/MP3", "C1C/MP1", "C1C/MP3", "C2/MP1",
            "C2/MP2", "C2/MP3", "C2B/MP1", "C2B/MP3", "C3/MP1", "C3/MP2", "C3/MP3", "C4/MP1",
            "C4/MP2", "C4/MP3", "C5/MP1", "C5/MP2", "C5/MP3",
        ]
    );

    // The decoded `targets.zrd` binds every slot. The binding is exactly the
    // mode family the objective descriptions name: a flag objective is
    // Capture the Flag, a zeppelin objective is Zeppelin vs. Zeppelin, and no
    // such objective is Deathmatch. No slot is unresolved in this
    // installation; the synthetic suite pins what keeps a slot unknown.
    let bindings: Vec<(String, ScenarioMode)> = catalog
        .slots
        .iter()
        .map(|slot| {
            let Resolved::Known(known) = &slot.mode else {
                panic!("{} has an unresolved mode: {:?}", slot.id, slot.mode);
            };
            (format!("{}/MP{}", slot.world_group, slot.slot), known.value)
        })
        .collect();
    assert_eq!(
        bindings,
        [
            ("C1/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C1/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C1/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C1B/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C1B/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C1C/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C1C/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C2/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C2/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C2/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C2B/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C2B/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C3/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C3/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C3/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C4/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C4/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C4/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
            ("C5/MP1".to_owned(), ScenarioMode::Deathmatch),
            ("C5/MP2".to_owned(), ScenarioMode::CaptureTheFlag),
            ("C5/MP3".to_owned(), ScenarioMode::ZeppelinVsZeppelin),
        ]
    );

    // Every binding cites the slot's own `targets.zrd` member and the
    // observed-tool class.
    for slot in &catalog.slots {
        let Resolved::Known(known) = &slot.mode else {
            panic!("{} is unresolved", slot.id);
        };
        assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        let span = known.provenance.source.as_ref().expect("a source span");
        assert_eq!(span.container_path(), slot.program.container_path());
        assert_eq!(
            slot.program.container_path().rsplit('/').next(),
            Some("zrdr.zbd")
        );
        assert_eq!(span.member_key(), Some("targets.zrd"));
        assert!(span.offset() > 0);
        assert!(span.length() > 0);
        assert!(span.member_sha256().is_some());
        assert!(slot.program.member_sha256().is_some());
    }

    // The whole-archive markers stay recorded, but they are the weaker,
    // mixed evidence: they are not one marker per mode family and disagree
    // with the decoded binding for several slots (for example C2/MP3 carries
    // the `Flag base` text while its objectives are zeppelins).
    let with = |marker: SlotMarker| -> Vec<String> {
        catalog
            .slots
            .iter()
            .filter(|slot| slot.markers.contains(&marker))
            .map(|slot| format!("{}/MP{}", slot.world_group, slot.slot))
            .collect()
    };
    assert_eq!(with(SlotMarker::FlagBase), ["C1/MP2", "C2/MP2", "C2/MP3"]);
    assert_eq!(
        with(SlotMarker::ZeppelinData),
        [
            "C1/MP3", "C1B/MP3", "C1C/MP3", "C2/MP3", "C2B/MP3", "C3/MP1", "C3/MP2", "C3/MP3",
            "C4/MP1", "C4/MP2", "C4/MP3", "C5/MP3",
        ]
    );
}
