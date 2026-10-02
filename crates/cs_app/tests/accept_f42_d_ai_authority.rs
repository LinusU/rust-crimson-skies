//! Task #465 acceptance tests: can an AI aircraft or another non-player
//! authority earn an original stunt?
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-D`. Required capabilities: retail. Task test prefix:
//! `accept_f42_d_ai_`.
//!
//! The question has one half that files can answer and one that they cannot.
//! This file pins the half that files can answer, and pins it as a
//! **measurement with numbers behind it**, because a survey that answered
//! `earning_authority_is_measured() == false` and nothing else would be
//! indistinguishable from a reader that never looked:
//!
//! * the objective **records** carry a complete key vocabulary, and
//!   `keys_naming_an_authority()` is derived from it rather than asserted — an
//!   authored record that *does* carry a `player_only` key must be found;
//! * the objective **state machine** carries the original's own stunt
//!   completion condition (`DANGER_ZONES_COMPLETED`, zone names and **no**
//!   subject) and its one actor-scoped condition (`TRAVELERS`, whose subject is
//!   `player`, a named non-player actor or an undecoded index);
//! * each instant-action scenario declares the non-player aircraft that share
//!   its stunt scenario, so the runtime's `AiFlight` refusal has a measured
//!   hazard behind it.
//!
//! The unignored tests author every byte (the `.zrd` grammar, a reader archive,
//! a world container) and run the production decoder, the production discovery
//! and the production survey. The `#[ignore]`d test reads the owner's
//! installation through the same production entry point and states the measured
//! corpus.
//!
//! Nothing here is `verified_original`: no original run happened, and reading
//! the installation's files is not evidence of how the game behaves.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use cs_app::stunts::{StuntAuthoritySurveyError, survey_retail_stunt_authority};
use cs_content::stunts::{
    DANGER_ZONE_LABEL_PREFIX, FLY_THROUGH_CATEGORY_LABEL, FLY_THROUGH_HELP_LABEL,
    OBJECTIVE_BLOCK_PREFIX, OBJECTIVE_DANGER_ZONE_COUNT_KEY, OBJECTIVE_DANGER_ZONES_KEY,
    OBJECTIVE_TRAVELERS_KEY, SCENARIO_MEMBER, SCENARIO_OBJECTIVES_MEMBER, SCENARIO_TARGETS_MEMBER,
    STUNT_MISSION_TYPE, TEAM_ONE_HELP_LABEL, TravellerSubject, ZrdValue, decode_zrd,
    fly_through_labelled_objectives, objective_record, objective_record_count,
    objective_record_keys, objective_state_machine, scenario_fly_through_targets,
    scenario_mission_type, scenario_non_player_aircraft, team_scoped_objectives, zrd_flat_fields,
};

// ------------------------------------------------------------- .zrd writer ---

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
/// the children (the measured `count - 1` grammar, F09/#463).
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A flat alternating key/value `.zrd` record, the shape `ia.zrd` and the
/// objective blocks use.
fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

/// One authored `targets.zrd` objective, a list of `[key, value]` pairs.
fn target_document(pairs: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zrd_list(
        pairs
            .into_iter()
            .map(|(key, value)| zrd_list(vec![zrd_text(key), value]))
            .collect(),
    )
}

/// A one-element list of a text node, the shape the original writes a scalar in
/// (`mission_type`, `player_plane`, `num_wingmen`, …).
fn zrd_text1(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

/// One authored `ia.zrd`: the scenario mode and its declared aircraft.
///
/// The measured shape is a flat alternating root; `group<N>` is itself a flat
/// alternating record.
fn scenario_document(mission_type: &str) -> Vec<u8> {
    let enemy = |count: u32, plane: &str, skill: &str| {
        zrd_flat(vec![
            ("num_enemies", zrd_list(vec![zrd_int(count)])),
            ("enemy_name", zrd_text1("MSG_VEH_BHAT_AUTOGYRO")),
            ("enemy_plane", zrd_text1(plane)),
            ("enemy_skill", zrd_text1(skill)),
        ])
    };
    zrd_flat(vec![
        ("mission_type", zrd_text1(mission_type)),
        ("player_plane", zrd_text1("Kestrel")),
        ("num_wingmen", zrd_list(vec![zrd_int(3)])),
        ("group1", enemy(6, "Autogyro", "novice")),
        ("group2", enemy(5, "Kestrel", "veteran")),
        ("ace_name", zrd_text1("MSG_SSCRAWFORD_NAME")),
        ("ace_plane", zrd_text1("Peacemaker")),
        ("ace_skill", zrd_text1("ace")),
    ])
}

/// One authored objective block of the state machine.
///
/// `danger_zones` is the original's own `DANGER_ZONES_COMPLETED` value: world
/// `dzpath<N>` names and **no subject**. `traveller` is the measured
/// `TRAVELERS` shape: subject, relation, target, distance, count.
fn objective_block(
    id: u32,
    danger_zones: &[&str],
    required: Option<u32>,
    traveller: Option<Vec<u8>>,
) -> (String, Vec<u8>) {
    let key = format!("{OBJECTIVE_BLOCK_PREFIX}{id}");
    let mut entries: Vec<(&str, Vec<u8>)> = Vec::new();
    entries.push(("BEGIN_DORMANT", zrd_list(vec![zrd_int(1)])));
    if !danger_zones.is_empty() {
        entries.push((
            OBJECTIVE_DANGER_ZONES_KEY,
            zrd_list(danger_zones.iter().map(|zone| zrd_text(zone)).collect()),
        ));
    }
    if let Some(required) = required {
        entries.push((
            OBJECTIVE_DANGER_ZONE_COUNT_KEY,
            zrd_list(vec![zrd_int(required)]),
        ));
    }
    if let Some(traveller) = traveller {
        entries.push((OBJECTIVE_TRAVELERS_KEY, traveller));
    }
    let body = zrd_flat(entries);
    (key, body)
}

/// An authored `objectives.zrd`: the measured one-element wrapper around one
/// flat record.
fn objectives_document(blocks: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("MISSION_TIMER", zrd_list(vec![zrd_text("0.0")])),
        ("PLAYER_INIT", zrd_list(vec![zrd_int(1)])),
    ];
    for (key, body) in &blocks {
        entries.push((key.as_str(), body.clone()));
    }
    zrd_list(vec![zrd_flat(entries)])
}

/// A `TRAVELERS` value naming `player` as the subject.
fn travellers_player(target: &str, distance: u32) -> Vec<u8> {
    zrd_list(vec![
        zrd_text("player"),
        zrd_text("APPROACHING"),
        zrd_text(target),
        zrd_int(distance),
        zrd_int(1),
    ])
}

// ------------------------------------------------------- reader-archive writer ---

/// A version-one reader archive holding `members` in order, as task #463's test
/// authored it.
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

// ------------------------------------------------------- world-container writer ---

/// One authored detection-zone node, as task #463's test wrote them.
struct ZoneNode<'a> {
    name: &'a str,
    mesh_index: i32,
    corners: [[f32; 3]; 2],
}

/// A synthetic CS GameZ container holding exactly the authored nodes. The
/// layout is the one #427's test authored and the production reader accepted.
fn world_container(nodes: &[ZoneNode<'_>]) -> Vec<u8> {
    const SIGNATURE: u32 = 43_455_010;
    const VERSION: u32 = 42;
    const NODES_OFFSET: u32 = 512;
    const SLOT: usize = 212;
    const OBJECT3D_BYTES: usize = 144;
    const OBJECT3D: u32 = 5;
    const IDENTITY: u32 = 40;

    let data_offset = NODES_OFFSET as usize + SLOT * nodes.len();
    let mut offsets = Vec::with_capacity(nodes.len());
    let mut cursor = data_offset;
    for _ in nodes {
        offsets.push(cursor);
        cursor += OBJECT3D_BYTES;
    }

    let mut bytes = vec![0_u8; cursor];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };

    for (field, value) in [
        (0_usize, SIGNATURE),
        (4, VERSION),
        (8, 0x1234_5678),
        (12, 1),
        (16, 40),
        (20, 248),
        (24, 256),
        (28, nodes.len() as u32),
        (32, 0),
        (36, NODES_OFFSET),
    ] {
        word(&mut bytes, field, value);
    }

    for (index, node) in nodes.iter().enumerate() {
        let at = NODES_OFFSET as usize + SLOT * index;
        let name = node.name.as_bytes();
        assert!(
            name.len() < 36,
            "the fixture's names fit their 36-byte field"
        );
        bytes[at..at + name.len()].copy_from_slice(name);
        word(&mut bytes, at + 36, 0x0180_001c);
        word(&mut bytes, at + 44, 1);
        word(&mut bytes, at + 48, 255);
        word(&mut bytes, at + 52, OBJECT3D);
        word(&mut bytes, at + 56, offsets[index] as u32);
        word(&mut bytes, at + 60, node.mesh_index as u32);
        word(&mut bytes, at + 68, 1);
        word(&mut bytes, at + 196, 160);
        word(&mut bytes, at + 208, 0x0200_0000 | index as u32);
        half(&mut bytes, at + 84, 0);
        half(&mut bytes, at + 86, 0);
        for (axis, value) in node.corners[0].iter().enumerate() {
            float(&mut bytes, at + 140 + 4 * axis, *value);
        }
        for (axis, value) in node.corners[1].iter().enumerate() {
            float(&mut bytes, at + 140 + 12 + 4 * axis, *value);
        }
        let data = offsets[index];
        word(&mut bytes, data, IDENTITY);
        for axis in 0..3 {
            float(&mut bytes, data + 36 + 4 * axis, 1.0);
        }
        for axis in 0..3 {
            float(&mut bytes, data + 48 + 12 * axis, 1.0);
            float(&mut bytes, data + 48 + 4 * axis + 4, 0.0);
            float(&mut bytes, data + 48 + 4 * axis + 8, 0.0);
        }
    }
    bytes
}

/// A throwaway installation tree.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t465-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ------------------------------------------------ the extraction, on authored bytes ---

/// The flat reader reads the measured alternating shape and nothing else. A
/// value that merely *looks* like a pair (`INACTIVE1 ["fuel_truck01", "tank"]`)
/// must not become a key, or the census would invent vocabulary out of the data.
#[test]
fn accept_f42_d_ai_the_flat_reader_reads_pairs_and_never_invents_a_key() {
    let record = decode_zrd(&zrd_flat(vec![
        ("MISSION_TIMER", zrd_list(vec![zrd_text("0.0")])),
        (
            "OBJECTIVE7",
            zrd_list(vec![zrd_text("fuel_truck01"), zrd_text("tank")]),
        ),
        ("REMOVE_OBJECTIVE_TARGET", zrd_text1("fuel_truck01")),
    ]))
    .expect("the authored record decodes");

    let fields = zrd_flat_fields(&record);
    let keys: Vec<&str> = fields.iter().map(|(key, _)| *key).collect();
    assert_eq!(
        keys,
        vec!["MISSION_TIMER", "OBJECTIVE7", "REMOVE_OBJECTIVE_TARGET"],
        "only the flat alternating keys, never a value that looks like a pair"
    );
    assert_eq!(
        fields[1].1.as_list().map(<[ZrdValue]>::len),
        Some(2),
        "the value is still the whole two-element list"
    );
    // A record whose value sits where a key belongs — here a list that looks
    // exactly like a `[key, value]` pair — is skipped, not read as a pair. A
    // shape-agnostic walk would invent the vocabulary the census reports.
    let dangling = decode_zrd(&zrd_list(vec![
        zrd_text("MISSION_TIMER"),
        zrd_text("0.0"),
        zrd_list(vec![zrd_text("fuel_truck01"), zrd_text("tank")]),
        zrd_text("PLAYER_INIT"),
        zrd_list(vec![zrd_int(1)]),
    ]))
    .expect("the dangling record decodes");
    let dangling_keys: Vec<&str> = zrd_flat_fields(&dangling)
        .iter()
        .map(|(key, _)| *key)
        .collect();
    assert_eq!(
        dangling_keys,
        vec!["MISSION_TIMER", "PLAYER_INIT"],
        "a value in a key position is never read as a key, and the pairing survives it"
    );
    assert!(
        zrd_flat_fields(&decode_zrd(&zrd_text("not a record")).expect("a text node decodes"))
            .is_empty(),
        "a text node carries no fields"
    );
}

/// The objective record inventory is **complete**: every key of every record is
/// counted, and the authority scan is derived from it, so an authored record
/// that does carry an actor key is found rather than argued away.
#[test]
fn accept_f42_d_ai_the_objective_record_inventory_is_complete_and_finds_an_actor_key() {
    let targets = decode_zrd(&zrd_list(vec![
        target_document(vec![
            ("description", zrd_text("MSG_TRGT_NYPD")),
            ("nodes", zrd_text1("dz1")),
            ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
            ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
        ]),
        target_document(vec![
            ("description", zrd_text("MSG_TRGT_FLAGBASE")),
            ("nodes", zrd_text1("ctf_1")),
            ("objective", zrd_list(vec![])),
            ("help_label", zrd_text(TEAM_ONE_HELP_LABEL)),
        ]),
        // A record that scopes its completion to an authority, and that carries
        // only the fly-through help label — the shape of the three campaign
        // records #463's stricter selector drops. The measured corpus has no
        // authority key at all; this one must be *found* so that "none" is a
        // measurement rather than a constant.
        target_document(vec![
            ("description", zrd_text("MSG_TRGT_GATED")),
            ("nodes", zrd_text1("dz2")),
            ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
            ("player_only", zrd_list(vec![zrd_int(1)])),
        ]),
    ]))
    .expect("the authored targets decode");

    assert_eq!(objective_record_count(&targets), 3);
    assert_eq!(
        objective_record_keys(&targets),
        vec![
            ("category_label".to_owned(), 1),
            ("description".to_owned(), 3),
            ("help_label".to_owned(), 3),
            ("nodes".to_owned(), 3),
            ("objective".to_owned(), 1),
            ("player_only".to_owned(), 1),
        ],
        "the whole vocabulary, sorted, with each key counted per record"
    );
    assert_eq!(scenario_fly_through_targets(&targets).len(), 1);
    assert_eq!(team_scoped_objectives(&targets), 1);

    // The looser reading: a record labelled by either measured label. The
    // stricter selector above needs a `category_label`, which the third record
    // does not carry, so the two readings differ by exactly one here — the shape
    // of the three campaign records that differ in the measured installation.
    assert_eq!(fly_through_labelled_objectives(&targets), 2);

    // The measured corpus's six keys contain no authority key; the authored one
    // does, and a survey reading this member must report it.
    let clean = decode_zrd(&zrd_list(vec![target_document(vec![
        ("description", zrd_text("MSG_TRGT_NYPD")),
        ("nodes", zrd_text1("dz1")),
        ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
        ("other_target", zrd_list(vec![])),
    ])]))
    .expect("the clean targets decode");
    let keys: Vec<String> = objective_record_keys(&clean)
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert!(
        !keys
            .iter()
            .any(|key| { cs_content::stunts::AUTHORITY_KEY_VOCABULARY.contains(&key.as_str()) }),
        "none of the measured keys is in the declared authority vocabulary"
    );
}

/// The state machine reader finds the original's own stunt condition and its one
/// actor-scoped condition, reads the subject of each, and does not decode an
/// index or a value that is not a condition.
#[test]
fn accept_f42_d_ai_the_state_machine_reads_stunt_conditions_and_traveller_subjects() {
    let machine = objective_state_machine(
        &decode_zrd(&objectives_document(vec![
            objective_block(1, &["dzpath1"], None, Some(travellers_player("dz1", 1000))),
            objective_block(2, &["dzpath7", "dzpath8"], Some(1), None),
            objective_block(
                3,
                &[],
                None,
                Some(zrd_list(vec![
                    zrd_text("wingman_3"),
                    zrd_text("LEAVING"),
                    zrd_text("player"),
                    zrd_int(2000),
                    zrd_int(1),
                ])),
            ),
            objective_block(
                4,
                &[],
                None,
                Some(zrd_list(vec![
                    zrd_int(1),
                    zrd_text("APPROACHING"),
                    zrd_text("cargozep2"),
                    zrd_int(200),
                    zrd_int(1),
                ])),
            ),
            objective_block(
                5,
                &[],
                None,
                Some(zrd_list(vec![zrd_list(vec![zrd_int(1), zrd_int(2)])])),
            ),
        ]))
        .expect("the authored state machine decodes"),
    );

    assert_eq!(machine.blocks(), 5);
    assert_eq!(
        machine.stunt_conditions().len(),
        2,
        "only the blocks that name danger zones are stunt conditions"
    );
    let first = &machine.stunt_conditions()[0];
    assert_eq!(first.objective(), "OBJECTIVE1");
    assert_eq!(first.zones(), ["dzpath1"]);
    assert_eq!(
        first.required_count(),
        None,
        "an absent count is not a zero"
    );
    let second = &machine.stunt_conditions()[1];
    assert_eq!(second.objective(), "OBJECTIVE2");
    assert_eq!(second.zones(), ["dzpath7", "dzpath8"]);
    assert_eq!(second.required_count(), Some(1));

    let subjects: Vec<&TravellerSubject> = machine
        .travellers()
        .iter()
        .map(|condition| condition.subject())
        .collect();
    assert_eq!(subjects.len(), 4, "one condition per TRAVELERS field");
    assert_eq!(subjects[0], &TravellerSubject::Player);
    assert_eq!(
        subjects[1],
        &TravellerSubject::Named("wingman_3".to_owned())
    );
    assert_eq!(subjects[2], &TravellerSubject::Indexed(1));
    assert_eq!(
        subjects[3],
        &TravellerSubject::Unreadable,
        "a subject of a shape this reader does not decode is reported, not dropped"
    );
    assert_eq!(machine.travellers()[0].relation(), Some("APPROACHING"));
    assert_eq!(machine.travellers()[0].target(), Some("dz1"));
    assert!(machine.travellers()[0].target_is_danger_zone_label());
    assert!(
        machine.travellers()[0]
            .target()
            .is_some_and(|target| { target.starts_with(DANGER_ZONE_LABEL_PREFIX) }),
        "the label prefix is the measured `dz…` spelling"
    );
    assert!(machine.travellers()[1].subject_is_non_player());
    assert!(!machine.travellers()[1].target_is_danger_zone_label());
    assert!(!machine.travellers()[0].subject_is_non_player());

    // The key vocabulary is complete: every field of every block is counted.
    let keys: Vec<&str> = machine.keys().iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "BEGIN_DORMANT",
            OBJECTIVE_DANGER_ZONES_KEY,
            OBJECTIVE_DANGER_ZONE_COUNT_KEY,
            OBJECTIVE_TRAVELERS_KEY,
        ],
        "the vocabulary is the union of the blocks' fields, sorted"
    );
    assert_eq!(
        machine.keys()[0].1,
        5,
        "BEGIN_DORMANT is in all five blocks"
    );

    // The measured wrapper: a document that is already a record needs no
    // wrapper, and one that is wrapped is read through it.
    let bare = decode_zrd(&zrd_flat(vec![("OBJECTIVE9", zrd_list(vec![]))]))
        .expect("a bare record decodes");
    assert_eq!(objective_state_machine(&bare).blocks(), 1);
    assert_eq!(
        objective_record(&bare).as_list().map(<[ZrdValue]>::len),
        Some(2)
    );
}

/// The scenario reader reports the non-player aircraft a scenario declares and
/// never invents a zero: a scenario that declares none says so.
#[test]
fn accept_f42_d_ai_the_scenario_reads_its_declared_non_player_aircraft() {
    let scenario =
        decode_zrd(&scenario_document(STUNT_MISSION_TYPE)).expect("the authored scenario decodes");
    assert_eq!(scenario_mission_type(&scenario), Some(STUNT_MISSION_TYPE));

    let aircraft = scenario_non_player_aircraft(&scenario);
    assert_eq!(aircraft.player_plane(), Some("Kestrel"));
    assert_eq!(aircraft.wingmen(), Some(3));
    assert!(aircraft.is_declared());
    assert_eq!(aircraft.enemy_groups().len(), 2);
    assert_eq!(aircraft.enemy_groups()[0].index(), 1);
    assert_eq!(aircraft.enemy_groups()[0].count(), Some(6));
    assert_eq!(aircraft.enemy_groups()[0].plane(), Some("Autogyro"));
    assert_eq!(aircraft.enemy_groups()[0].skill(), Some("novice"));
    assert_eq!(
        aircraft.enemy_groups()[0].name_label(),
        Some("MSG_VEH_BHAT_AUTOGYRO")
    );
    assert_eq!(aircraft.enemy_groups()[1].plane(), Some("Kestrel"));
    assert_eq!(
        aircraft.summed_enemy_group_counts(),
        11,
        "the sum is over the authored group counts only: the wingmen and the ace are not in it"
    );
    assert_eq!(aircraft.ace().plane(), Some("Peacemaker"));
    assert_eq!(aircraft.ace().skill(), Some("ace"));
    assert!(aircraft.ace().is_declared());

    // A scenario that declares no aircraft at all is an empty measurement, not
    // a zero-filled one.
    let bare = decode_zrd(&zrd_flat(vec![("mission_type", zrd_text1("zeppelin_run"))]))
        .expect("the bare scenario decodes");
    let bare_aircraft = scenario_non_player_aircraft(&bare);
    assert!(!bare_aircraft.is_declared());
    assert_eq!(bare_aircraft.wingmen(), None);
    assert!(bare_aircraft.enemy_groups().is_empty());
    assert!(!bare_aircraft.ace().is_declared());

    // A field that merely starts with the group prefix is not a group.
    let noisy = decode_zrd(&zrd_flat(vec![
        ("group_x", zrd_list(vec![zrd_int(9)])),
        ("group3", zrd_list(vec![zrd_int(2)])),
    ]))
    .expect("the noisy scenario decodes");
    let noisy_aircraft = scenario_non_player_aircraft(&noisy);
    assert_eq!(
        noisy_aircraft.enemy_groups().len(),
        1,
        "`group_x` is not an enemy group record"
    );
    assert_eq!(noisy_aircraft.enemy_groups()[0].index(), 3);
    assert!(
        !noisy_aircraft.is_declared(),
        "a group record with no count, plane or skill declares nothing measurable"
    );
}

// -------------------------------------------------- the survey over a file ----

/// The whole production survey over an authored installation: an
/// instant-action reader with a scenario descriptor, objective records and an
/// objective machine; a campaign reader with team-scoped objectives, a
/// non-player `TRAVELERS` subject and a stunt condition; and a reader that
/// declares no objectives at all.
#[test]
fn accept_f42_d_ai_the_survey_measures_the_authority_surface_of_every_reader() {
    let install = TempInstallation::new("surface");
    install.write(
        "ZBD/C5/gamez.zbd",
        &world_container(&[ZoneNode {
            name: "dzpath1",
            mesh_index: 949,
            corners: [[0.0, 0.0, 0.0], [8.0, 8.0, 8.0]],
        }]),
    );
    install.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[
            (SCENARIO_MEMBER, scenario_document(STUNT_MISSION_TYPE)),
            (
                SCENARIO_TARGETS_MEMBER,
                zrd_list(vec![
                    target_document(vec![
                        ("description", zrd_text("MSG_TRGT_NYPD")),
                        ("nodes", zrd_text1("dz1")),
                        ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
                        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
                    ]),
                    target_document(vec![
                        ("description", zrd_text("MSG_TRGT_ZEP_ENEMY")),
                        ("nodes", zrd_text1("multiplayer1zep")),
                        ("category_label", zrd_text("MSG_OBJ_ZEPPELIN")),
                        ("help_label", zrd_text("MSG_OBJ_DISABLEENG")),
                    ]),
                ]),
            ),
            (
                SCENARIO_OBJECTIVES_MEMBER,
                objectives_document(vec![objective_block(
                    1,
                    &["dzpath1"],
                    None,
                    Some(travellers_player("dz1", 500)),
                )]),
            ),
        ]),
    );
    install.write(
        "ZBD/C5/M01/zrdr.zbd",
        &reader_archive(&[
            (
                SCENARIO_TARGETS_MEMBER,
                zrd_list(vec![
                    target_document(vec![
                        ("description", zrd_text("MSG_TRGT_FLAGBASE")),
                        ("nodes", zrd_text1("ctf_1")),
                        ("objective", zrd_list(vec![])),
                        ("help_label", zrd_text(TEAM_ONE_HELP_LABEL)),
                    ]),
                    target_document(vec![
                        ("description", zrd_text("MSG_TRGT_SEAPLANE_HANGER")),
                        ("nodes", zrd_text1("sghangar")),
                        ("objective", zrd_list(vec![])),
                        ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
                        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
                    ]),
                    // A record that scopes its completion to an authority. The
                    // measured corpus carries none, and the survey has to be
                    // able to *say so*: a scan hard-wired to report "none"
                    // would pass every measured assertion and be worthless.
                    target_document(vec![
                        ("description", zrd_text("MSG_TRGT_GATED")),
                        ("nodes", zrd_text1("dz2")),
                        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
                        ("player_only", zrd_list(vec![zrd_int(1)])),
                    ]),
                ]),
            ),
            (
                SCENARIO_OBJECTIVES_MEMBER,
                objectives_document(vec![
                    objective_block(
                        2,
                        &["dzpath3"],
                        Some(2),
                        Some(zrd_list(vec![
                            zrd_text("secfury_5"),
                            zrd_text("APPROACHING"),
                            zrd_text("dz2"),
                            zrd_int(300),
                            zrd_int(1),
                        ])),
                    ),
                    objective_block(
                        3,
                        &[],
                        None,
                        Some(zrd_list(vec![
                            zrd_text("player"),
                            zrd_text("LEAVING"),
                            zrd_text("cargozep2"),
                            zrd_int(2000),
                            zrd_int(1),
                        ])),
                    ),
                ]),
            ),
        ]),
    );
    // A reader archive with no objective member at all: a measured absence, not
    // a skipped row.
    install.write("ZBD/C5/M02/zrdr.zbd", &reader_archive(&[]));

    let survey = survey_retail_stunt_authority(&install.root)
        .unwrap_or_else(|error| panic!("surveys: {error}"));

    assert_eq!(survey.install_sha256().len(), 64);
    assert_eq!(survey.len(), 3, "every reader archive is a row");
    assert!(!survey.is_empty());

    // Objective records.
    assert_eq!(
        survey.objective_records(),
        5,
        "2 + 3, the empty reader has none"
    );
    assert_eq!(
        survey.fly_through_objectives(),
        2,
        "one fly-through target per reader that declares one"
    );
    assert_eq!(
        survey.fly_through_labelled_objectives(),
        3,
        "the gated record carries only the help label, so the looser reading sees one more - the \
         shape of the three campaign records that differ in the measured installation"
    );
    assert_eq!(survey.team_scoped_objectives(), 1);
    assert_eq!(survey.objective_blocks(), 3);
    assert_eq!(survey.scenarios().count(), 1);
    assert_eq!(survey.scenarios_declaring_non_player_aircraft().count(), 1);
    assert_eq!(survey.stunt_flying_scenarios().count(), 1);

    // The key vocabulary is complete over both surfaces, and the authority scan
    // is derived from it.
    let vocabulary = survey.objective_keys();
    let keys: Vec<&str> = vocabulary.iter().map(|(key, _)| key.as_str()).collect();
    for expected in [
        "category_label",
        "description",
        "help_label",
        "nodes",
        "objective",
        OBJECTIVE_DANGER_ZONE_COUNT_KEY,
        OBJECTIVE_DANGER_ZONES_KEY,
        OBJECTIVE_TRAVELERS_KEY,
    ] {
        assert!(keys.contains(&expected), "the vocabulary has {expected}");
    }
    assert_eq!(
        survey.keys_naming_an_authority(),
        vec![("zbd/c5/m01/zrdr.zbd".to_owned(), "player_only".to_owned())],
        "the one authored authority key is found with its own row named, so the measured corpus \
         carrying none is a measurement and not a hard-wired answer"
    );

    // The campaign reader's `dz2` target is a non-player subject on a zone
    // label: the survey reports it, because hiding it would be the failure.
    assert_eq!(survey.traveller_subject_census().player, 2);
    assert_eq!(survey.traveller_subject_census().named, 1);
    assert_eq!(survey.traveller_subject_census().indexed, 0);
    assert_eq!(survey.non_player_subjects(), vec!["secfury_5"]);
    let danger_zone = survey
        .traveller_conditions_naming_a_danger_zone()
        .collect::<Vec<_>>();
    assert_eq!(danger_zone.len(), 2, "the `dz1` and `dz2` targets");
    assert_eq!(
        survey
            .non_player_danger_zone_conditions()
            .map(|condition| condition.target().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["dz2"],
        "the one non-player subject that names a zone label, reported not hidden"
    );

    // Stunt conditions carry zones and a count, and no subject at all.
    let conditions = survey.stunt_conditions().collect::<Vec<_>>();
    assert_eq!(conditions.len(), 2);
    assert_eq!(conditions[0].zones(), ["dzpath1"]);
    assert_eq!(conditions[0].required_count(), None);
    assert_eq!(conditions[1].zones(), ["dzpath3"]);
    assert_eq!(conditions[1].required_count(), Some(2));

    // The scenario row carries the measured aircraft and its provenance.
    let scenario_row = survey
        .rows()
        .iter()
        .find(|row| row.container() == "zbd/c5/ia1/zrdr.zbd")
        .expect("the c5 instant-action reader is a row");
    let scenario = scenario_row
        .scenario()
        .expect("the instant-action reader carries a scenario descriptor");
    assert_eq!(scenario.mission_type(), STUNT_MISSION_TYPE);
    assert_eq!(scenario.span().member(), SCENARIO_MEMBER);
    assert_eq!(scenario.span().container(), "zbd/c5/ia1/zrdr.zbd");
    assert_eq!(scenario.span().container_sha256().len(), 64);
    assert!(scenario.span().length() > 0);
    assert_eq!(scenario.aircraft().wingmen(), Some(3));
    assert_eq!(scenario.aircraft().summed_enemy_group_counts(), 11);

    // The campaign row's provenance is its own objective members.
    let campaign = survey
        .rows()
        .iter()
        .find(|row| row.container() == "zbd/c5/m01/zrdr.zbd")
        .expect("the c5 campaign reader is a row");
    assert!(campaign.scenario().is_none(), "a mission has no ia.zrd");
    let corpus = campaign
        .objectives()
        .expect("a mission declares objectives");
    assert_eq!(corpus.span().member(), SCENARIO_TARGETS_MEMBER);
    assert_eq!(corpus.count(), 3);
    assert_eq!(
        corpus.authority_keys(),
        vec!["player_only"],
        "the corpus carries the authored authority key, and it is visible here"
    );
    let machine = campaign.machine().expect("a mission declares objectives");
    assert_eq!(machine.span().member(), SCENARIO_OBJECTIVES_MEMBER);
    assert_eq!(machine.machine().blocks(), 2);

    // The reader with no objective member is a measured absence, not a skipped
    // row.
    let empty = survey
        .rows()
        .iter()
        .find(|row| row.container() == "zbd/c5/m02/zrdr.zbd")
        .expect("the empty reader is a row");
    assert!(empty.objectives().is_none());
    assert!(empty.machine().is_none());
    assert!(empty.scenario().is_none());

    // The rule itself is unmeasured, and the survey says so instead of letting
    // "no authority key" stand in for an answer.
    assert!(!survey.earning_authority_is_measured());
}

/// The survey's own refusals, over files: an undecodable member and an
/// installation with no world group are each a **named** refusal rather than a
/// shorter row list.
#[test]
fn accept_f42_d_ai_every_authority_refusal_names_the_container_and_member() {
    let garbage = TempInstallation::new("garbage");
    garbage.write(
        "ZBD/C5/gamez.zbd",
        &world_container(&[ZoneNode {
            name: "dzpath1",
            mesh_index: 949,
            corners: [[0.0; 3], [8.0, 8.0, 8.0]],
        }]),
    );
    garbage.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[
            (SCENARIO_MEMBER, scenario_document(STUNT_MISSION_TYPE)),
            (SCENARIO_TARGETS_MEMBER, vec![0_u8; 12]),
        ]),
    );
    match survey_retail_stunt_authority(&garbage.root) {
        Err(StuntAuthoritySurveyError::Decode {
            container,
            member,
            code,
            ..
        }) => {
            assert_eq!(container, "zbd/c5/ia1/zrdr.zbd");
            assert_eq!(member, SCENARIO_TARGETS_MEMBER);
            assert_eq!(code, "unknown_tag");
        }
        other => panic!("a non-.zrd member must be refused, got {other:?}"),
    }

    // An installation with reader archives but no scenario descriptor at all:
    // the aircraft half of the measurement would be empty for want of data.
    let scenarioless = TempInstallation::new("scenarioless");
    scenarioless.write("ZBD/C5/gamez.zbd", &world_container(&[]));
    scenarioless.write(
        "ZBD/C5/M01/zrdr.zbd",
        &reader_archive(&[(
            SCENARIO_TARGETS_MEMBER,
            zrd_list(vec![target_document(vec![
                ("description", zrd_text("MSG_TRGT_FLAGBASE")),
                ("nodes", zrd_text1("ctf_1")),
            ])]),
        )]),
    );
    assert!(
        matches!(
            survey_retail_stunt_authority(&scenarioless.root),
            Err(StuntAuthoritySurveyError::NoScenarioReaders)
        ),
        "a measurement with no scenario in it must refuse rather than report nothing"
    );

    // No world group at all.
    let bare = TempInstallation::new("bare");
    bare.write("ZBD/planes.zbd", b"authored fixture");
    assert!(
        matches!(
            survey_retail_stunt_authority(&bare.root),
            Err(StuntAuthoritySurveyError::NoWorldGroups)
        ),
        "an installation with no world group has nothing to measure"
    );
}

// ------------------------------------------------------------------ retail ---

/// The retail root, or a loud failure. The tests that call this are `#[ignore]`d;
/// a test that skipped itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The installation fingerprint of the owner's installation, recorded in
/// `docs/findings/2026-10-02-t465-ai-stunt-earning.md`.
const RETAIL_INSTALL_SHA256: &str =
    "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The measured corpus, through the production survey.
///
/// Measured over the owner's installation: **62** reader archives walked, **53**
/// of them carrying **332** objective records whose complete key vocabulary is
/// six keys and **none** of them names an earning authority. **67** of the
/// records are labelled fly-through danger-zone targets (67 by either label, 64 by#463's stricter selector), **46** are team-scoped. The
/// objective state machines declare **1 338** numbered blocks, of which **31**
/// carry the original's own stunt completion condition (`DANGER_ZONES_COMPLETED`
/// — zone names, no subject) and **75** carry the one actor-scoped condition
/// (`TRAVELERS`), whose subjects are `player` **43**, a named non-player actor
/// **26** and an undecoded index **6**. Every measured stunt condition is
/// therefore either anonymous or names `player`. All eight instant-action
/// scenarios declare non-player aircraft, the four `stunt_flying` ones among
/// them.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f42_d_ai_retail_no_measured_file_names_the_actor_that_earns_a_stunt() {
    let survey = survey_retail_stunt_authority(&retail_root()).expect("the retail corpus surveys");

    assert_eq!(
        survey.install_sha256(),
        RETAIL_INSTALL_SHA256,
        "the installation fingerprint the measurement was taken over"
    );
    assert_eq!(survey.len(), 62, "every reader archive in the installation");
    assert_eq!(survey.objective_records(), 332);
    assert_eq!(survey.fly_through_objectives(), 64);
    assert_eq!(
        survey.fly_through_labelled_objectives(),
        67,
        "three campaign records carry the fly-through help label and no category label"
    );
    // Those three are named, so the difference is traceable rather than a total:
    // #463's selector needs a `category_label`, and these records carry none.
    let only_labelled: Vec<&str> = survey
        .rows()
        .iter()
        .filter_map(|row| {
            let corpus = row.objectives()?;
            (corpus.fly_through_labelled() > corpus.fly_through()).then_some(row.container())
        })
        .collect();
    assert_eq!(
        only_labelled,
        vec![
            "zbd/c1/m02/zrdr.zbd",
            "zbd/c4/m03/zrdr.zbd",
            "zbd/c5/m02/zrdr.zbd"
        ],
        "the three readers whose help-labelled fly-through records #463's selector drops"
    );
    assert_eq!(survey.team_scoped_objectives(), 46);
    assert_eq!(survey.objective_blocks(), 1_338);

    // The complete objective key vocabulary: six keys, summed over both the
    // records and the machine blocks.
    let vocabulary = survey.objective_keys();
    let keys: Vec<(&str, u32)> = vocabulary
        .iter()
        .map(|(key, count)| (key.as_str(), *count))
        .collect();
    assert!(
        keys.contains(&("category_label", 146))
            && keys.contains(&("description", 331))
            && keys.contains(&("help_label", 294))
            && keys.contains(&("nodes", 331))
            && keys.contains(&("objective", 93))
            && keys.contains(&("other_target", 52)),
        "the six measured record keys with their counts: {keys:?}"
    );
    assert!(
        keys.contains(&(OBJECTIVE_DANGER_ZONES_KEY, 31)),
        "the original's own stunt completion condition is present"
    );
    assert!(
        !keys
            .iter()
            .any(|(key, _)| { cs_content::stunts::AUTHORITY_KEY_VOCABULARY.contains(key) }),
        "no measured objective key or machine key names an earning authority: {keys:?}"
    );
    assert!(
        survey.keys_naming_an_authority().is_empty(),
        "the survey's own scan agrees with the vocabulary"
    );

    // The stunt conditions: zones and a count, never a subject.
    let conditions = survey.stunt_conditions().collect::<Vec<_>>();
    assert_eq!(conditions.len(), 31);
    assert_eq!(
        conditions
            .iter()
            .filter(|condition| condition.required_count().is_some())
            .count(),
        6,
        "six blocks declare a required zone count"
    );
    for condition in &conditions {
        assert!(
            !condition.zones().is_empty(),
            "a stunt condition names at least one world zone"
        );
        assert!(
            condition
                .zones()
                .iter()
                .all(|zone| zone.starts_with("dzpath")),
            "and only `dzpath<N>` world zones: {:?}",
            condition.zones()
        );
    }
    // C2/M03's chain: the seaplane hangar gate completes, its target is removed
    // and the next gate is added. One named row, so the shape is asserted.
    let hangar = conditions
        .iter()
        .find(|condition| condition.zones() == ["dzpath1"])
        .expect("a campaign mission declares a dzpath1 stunt condition");
    assert_eq!(hangar.objective(), "OBJECTIVE17");

    // The actor-scoped conditions: the census, the named non-player subjects
    // and the three zone-label targets — all `player`.
    assert_eq!(survey.traveller_subject_census().player, 43);
    assert_eq!(survey.traveller_subject_census().named, 26);
    assert_eq!(survey.traveller_subject_census().indexed, 6);
    assert_eq!(survey.traveller_subject_census().unreadable, 0);
    let subjects = survey.non_player_subjects();
    assert_eq!(subjects.len(), 25, "25 distinct non-player subject names");
    for expected in ["secfury_5", "wingman_3", "devastator_1", "geminizep"] {
        assert!(
            subjects.contains(&expected),
            "the original names non-player actors: {expected}"
        );
    }
    assert!(
        survey
            .non_player_danger_zone_conditions()
            .collect::<Vec<_>>()
            .is_empty(),
        "no measured actor-scoped condition names a non-player subject on a zone label"
    );
    let zone_targets = survey
        .traveller_conditions_naming_a_danger_zone()
        .collect::<Vec<_>>();
    assert_eq!(zone_targets.len(), 3);
    for condition in &zone_targets {
        assert_eq!(condition.subject(), &TravellerSubject::Player);
        assert_eq!(condition.relation(), Some("APPROACHING"));
        assert_eq!(condition.target(), Some("dz1"));
    }

    // The scenarios: every one declares non-player aircraft, and every
    // `stunt_flying` scenario declares them next to its stunt zones.
    let scenarios = survey.scenarios().count();
    assert_eq!(scenarios, 8, "the eight instant-action scenarios");
    assert_eq!(
        survey.scenarios_declaring_non_player_aircraft().count(),
        8,
        "every scenario declares enemy groups at least"
    );
    let stunt_worlds: BTreeSet<String> = survey
        .rows()
        .iter()
        .filter(|row| {
            row.scenario()
                .is_some_and(|scenario| scenario.mission_type() == STUNT_MISSION_TYPE)
        })
        .map(|row| {
            row.container()
                .split('/')
                .nth(1)
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        stunt_worlds,
        ["c1b", "c2", "c4", "c5"]
            .iter()
            .map(|world| (*world).to_owned())
            .collect::<BTreeSet<_>>(),
        "the original marks four of the eight scenarios `stunt_flying`"
    );
    for row in survey.rows() {
        let Some(scenario) = row.scenario() else {
            continue;
        };
        if scenario.mission_type() != STUNT_MISSION_TYPE {
            continue;
        }
        let aircraft = scenario.aircraft();
        assert_eq!(
            aircraft.wingmen(),
            Some(3),
            "{} declares three AI wingmen",
            row.container()
        );
        assert_eq!(
            aircraft.enemy_groups().len(),
            4,
            "{} declares four enemy groups",
            row.container()
        );
        assert!(
            (12..=18).contains(&aircraft.summed_enemy_group_counts()),
            "and 12 to 18 enemies: {}",
            row.container()
        );
        assert!(aircraft.ace().is_declared());
    }

    // The rule itself remains unmeasured: the data names no actor for a stunt,
    // which is not the same as the original never crediting one.
    assert!(!survey.earning_authority_is_measured());
}
