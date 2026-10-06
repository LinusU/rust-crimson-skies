//! Acceptance for `M01-LC-DIRECTIVE-D` (#682): the findings document
//! `docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`
//! measures what M01's audio/UI/timer directives do, out of
//! `crimson.decrypted.exe`.
//!
//! The native-side claims (handler sites, field offsets, the mission-timer
//! start rule `value > 0.0f`, the two localized ids the timeout asks for) are
//! **static code evidence** and cannot be re-derived by a test. What these
//! tests do pin is every claim the document makes about **M01's own data** —
//! the block/site/key census, the spellings, the counts, the sound-group
//! vocabularies, the two `SET_HELP_LABEL` sites, the `MISSION_TIMER` value,
//! the `IDENTITY` roles and the names M01 spells in both roles — each read
//! out of the owner's installation by production code.
//!
//! The pinned values below *are* the document's figures, so a change in the
//! data fails the run and a document edited away from its own figures fails
//! review: the suite reads the installation, never the markdown, and does not
//! notice the document alone changing.
//!
//! Every test needs `CS_GAME_DIR`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::{CONTROL_RECORD_KEY_VOCABULARY, DecodedMember, control_member};
use cs_content::objectives::{
    OBJECTIVE_COMPLETED_SOUND_GROUP_KEY, OBJECTIVE_WAKEUP_SOUND_GROUP_KEY,
    measure_dormant_declarations,
};
use cs_content::stunts::{
    SCENARIO_OBJECTIVES_MEMBER, ZrdValue, decode_zrd, objective_record, zrd_directive_fields,
    zrd_flat_fields,
};

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail acceptance needs the install"),
    )
}

/// M01's control member, decoded by the production reader.
fn m01_control_document(dir: &std::path::Path) -> ZrdValue {
    let found = cs_assets::install::discover(dir).expect("production discovery reads the install");
    let spelling = "zbd/c1c/m01/zrdr.zbd";
    let bytes =
        std::fs::read(found.manifest.host_root.join(spelling)).expect("M01's reader archive");
    let path = cs_types::install::RelativePath::new(spelling).expect("a relative path");
    let discovery = cs_formats::script_raw::discover_container(spelling, &path, &bytes);
    let members: Vec<DecodedMember> = discovery
        .programs()
        .iter()
        .filter_map(|program| {
            let locator = program.locator();
            let name = locator.member()?;
            let document = decode_zrd(program.bytes()).expect("a member decodes");
            Some(DecodedMember::new(name.to_owned(), document))
        })
        .collect();
    assert!(
        members
            .iter()
            .any(|member| member.name == SCENARIO_OBJECTIVES_MEMBER),
        "M01 declares the control member the census names"
    );
    control_member("zbd/c1c/m01/zrdr.zbd", &members)
        .expect("M01 declares exactly one control member")
        .document
        .clone()
}

/// The sound groups M01's blocks declare, per block number.
fn m01_sound_groups(
    document: &ZrdValue,
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    BTreeMap<String, String>,
) {
    let blocks = measure_dormant_declarations(document).expect("every M01 block reads");
    let mut wakeup = BTreeMap::new();
    let mut completed = BTreeMap::new();
    let mut stopped = BTreeMap::new();
    for block in blocks {
        if let Some(name) = &block.wakeup_sound_group {
            wakeup.insert(block.block.clone(), name.clone());
        }
        if let Some(name) = &block.completed_sound_group {
            completed.insert(block.block.clone(), name.clone());
        }
        for (key, value) in zrd_directive_fields(
            zrd_flat_fields(objective_record(document))
                .into_iter()
                .find(|(key, _)| *key == block.block)
                .map_or(&ZrdValue::List(Vec::new()), |(_, value)| value),
        ) {
            if key != "STOP_QUEUED_SOUNDS" {
                continue;
            }
            for child in value.as_list().unwrap_or_default() {
                if let Some(name) = child.as_text() {
                    stopped.insert(block.block.clone(), name.to_owned());
                }
            }
        }
    }
    (wakeup, completed, stopped)
}

/// The four sound/help keys `M01-LC-DIRECTIVE-D` measures, with the census
/// figures its findings document's table records: blocks, sites and the
/// production census's own shape labels.
const M01_AUDIO_UI_KEYS: &[(&str, u32, u32, &[&str])] = &[
    ("COMPLETED_SOUND_GROUP", 23, 23, &["[text]x23"]),
    (
        "SET_HELP_LABEL",
        2,
        2,
        &["[text,text]x1", "[[text,text],text]x1"],
    ),
    ("STOP_QUEUED_SOUNDS", 3, 3, &["[text]x3"]),
    ("WAKEUP_SOUND_GROUP", 18, 18, &["[text]x18"]),
];

/// The keys of these families the document measures as **not** spelled by M01:
/// the seven mission-level `*_SOUND` record keys, the four timer keys and
/// `START_TAXI`. `DELETE_ON_SUCCESS` is in the list because the document
/// measures it as a `TRAVELERS` argument token rather than a directive key,
/// and `SLEEP_ANIM` because it is the one stage-A sibling in the same
/// not-spelled list that belongs to no family this document measures — it is
/// pinned here so the vocabulary walk keeps excluding it.
const NOT_SPELLED_BY_M01: &[&str] = &[
    "ADJUST_TIMER_WHEN_I_COMPLETE",
    "DELETE_ON_SUCCESS",
    "END_TIMER",
    "MISSION_LOST_SOUND",
    "MISSION_WON_SOUND",
    "OBJECTIVES_LOST_SOUND",
    "OBJECTIVES_WON_SOUND",
    "PRIMARY_COMPLETE_SOUND",
    "RESET_TIMER",
    "SECONDARY_COMPLETE_SOUND",
    "SLEEP_ANIM",
    "START_TAXI",
    "TERTIARY_COMPLETE_SOUND",
    "TIMER_ADJUST",
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_m01_spellings_of_the_audio_ui_timer_keys_match_the_document() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");

    let measured: BTreeMap<String, (u32, u32, Vec<String>)> = record
        .keys()
        .iter()
        .map(|key| {
            let shapes: Vec<String> = key
                .shapes
                .iter()
                .map(|(shape, count)| format!("{}x{}", shape.label(), count))
                .collect();
            (key.key.clone(), (key.blocks, key.sites, shapes))
        })
        .collect();

    assert_eq!(
        M01_AUDIO_UI_KEYS.len(),
        measured
            .keys()
            .filter(|key| M01_AUDIO_UI_KEYS.iter().any(|(spelled, ..)| spelled == key))
            .count(),
        "the document's table names every audio/UI key M01 spells"
    );
    for (spelled, blocks, sites, shapes) in M01_AUDIO_UI_KEYS {
        assert_eq!(
            measured.get(*spelled),
            Some(&(
                *blocks,
                *sites,
                shapes.iter().map(|shape| (*shape).to_owned()).collect()
            )),
            "measured blocks/sites/shapes for {spelled}"
        );
    }

    let spelled: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    for key in NOT_SPELLED_BY_M01 {
        assert!(
            !spelled.contains(key),
            "{key} is measured as not spelled by M01"
        );
    }
    // The document's claim that M01 spells none of the mission-level `*_SOUND`
    // record keys is a claim about CONTROL_RECORD_KEY_VOCABULARY too: the
    // record-level keys M01 does carry are exactly the five measured ones.
    assert_eq!(CONTROL_RECORD_KEY_VOCABULARY.len(), 5);
    for field in record.record_fields() {
        assert!(
            CONTROL_RECORD_KEY_VOCABULARY.contains(&field.0.key()),
            "{} is one of the five measured record fields",
            field.0.key()
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_the_sound_group_vocabularies_m01_declares_are_the_measured_lists() {
    let document = m01_control_document(&game_dir());

    // The two key constants the production reader uses for exactly these two
    // keys, so the test cannot silently disagree with it about the spellings.
    assert_eq!(OBJECTIVE_WAKEUP_SOUND_GROUP_KEY, "WAKEUP_SOUND_GROUP");
    assert_eq!(OBJECTIVE_COMPLETED_SOUND_GROUP_KEY, "COMPLETED_SOUND_GROUP");

    let (wakeup, completed, _stopped) = m01_sound_groups(&document);
    assert_eq!(wakeup.len(), 18, "18 blocks spell WAKEUP_SOUND_GROUP");
    assert_eq!(completed.len(), 23, "23 blocks spell COMPLETED_SOUND_GROUP");

    let distinct = |map: &BTreeMap<String, String>| -> Vec<String> {
        let mut names: Vec<String> = map.values().cloned().collect();
        names.sort();
        names.dedup();
        names
    };
    assert_eq!(
        distinct(&wakeup),
        vec![
            "music_battlesuccess_sg",
            "music_missionsuccess_sg",
            "music_prebattle_sg",
            "music_primaryobj_sg",
            "music_secondaryobj_sg",
            "snd_NW1Fass",
            "snd_NW1Start",
            "snd_c2-NW-m1_Jack_21",
            "snd_c2-NW-m1_Jack_34",
            "snd_c2-NW-m1_Jack_7",
            "snd_c2-NW-m1_Tex_35",
            "snd_c2-NW-m1_WorkersVoyage_12",
            "snd_c2-NW-m1_WorkersVoyage_13",
        ],
        "the 13 distinct wakeup sound groups the document lists"
    );
    assert_eq!(
        distinct(&completed),
        vec![
            "snd_NW1BSwan",
            "snd_NW1FirstHook",
            "snd_NW1Prim2",
            "snd_NW1Prim5Suc",
            "snd_NW1Sec1Suc",
            "snd_c2-NW-m1_Jack_11",
            "snd_c2-NW-m1_Jack_44",
            "snd_c2-NW-m1_Sparks_24",
            "snd_c2-NW-m1_Sparks_3",
            "snd_c2-NW-m1_Sparks_4",
            "snd_c2-NW-m1_Sparks_40",
            "snd_c2-NW-m1_Sparks_6",
            "snd_c2-NW-m1_Tex_17",
            "snd_c2-NW-m1_Tex_18",
            "snd_c2-NW-m1_Tex_5",
            "snd_c2-NW-m1_WorkersVoyage_15",
            "snd_c2-NW-m1_WorkersVoyage_19",
            "snd_c2-NW-m1_Zachary_14",
            "snd_c2-NW-m1_Zachary_20",
            "snd_c2-NW-m1_Zachary_37",
            "snd_c2-NW-m1_Zachary_45",
        ],
        "the 21 distinct completed sound groups the document lists"
    );
    assert_eq!(
        completed
            .values()
            .filter(|name| *name == "snd_NW1Prim2")
            .count(),
        2,
        "snd_NW1Prim2 is the one completed group spelled twice (OBJECTIVE3, OBJECTIVE48)"
    );
    assert_eq!(
        completed
            .values()
            .filter(|name| *name == "snd_c2-NW-m1_Jack_11")
            .count(),
        2,
        "snd_c2-NW-m1_Jack_11 is the other repeated completed group (OBJECTIVE42, OBJECTIVE43)"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_the_three_names_m01_plays_and_stops_are_the_measured_three() {
    let document = m01_control_document(&game_dir());
    let (wakeup, completed, stopped) = m01_sound_groups(&document);

    assert_eq!(
        stopped,
        BTreeMap::from([
            ("OBJECTIVE11".to_owned(), "snd_c2-NW-m1_Jack_21".to_owned()),
            ("OBJECTIVE15".to_owned(), "snd_c2-NW-m1_Tex_35".to_owned()),
            (
                "OBJECTIVE55".to_owned(),
                "snd_c2-NW-m1_Zachary_14".to_owned()
            ),
        ]),
        "STOP_QUEUED_SOUNDS names one queued sound per site, as the document records"
    );

    // Each stopped name is also a cue M01 plays somewhere, which is what makes
    // the document's "stop this name, not everything this objective started"
    // reading a measured observation rather than a reading of the key's name.
    let played: Vec<&String> = wakeup.values().chain(completed.values()).collect();
    for name in stopped.values() {
        assert!(
            played.contains(&name),
            "{name} is played by another M01 objective as well as stopped"
        );
    }
    assert_eq!(
        wakeup
            .iter()
            .filter(|(_, name)| ["snd_c2-NW-m1_Jack_21", "snd_c2-NW-m1_Tex_35"]
                .contains(&name.as_str()))
            .map(|(block, _)| block.as_str())
            .collect::<Vec<_>>(),
        vec![
            "OBJECTIVE29",
            "OBJECTIVE30",
            "OBJECTIVE31",
            "OBJECTIVE33",
            "OBJECTIVE34",
            "OBJECTIVE35"
        ],
        "the five dormant blocks that wake the two stopped cue names"
    );
    assert_eq!(
        completed.get("OBJECTIVE6").map(String::as_str),
        Some("snd_c2-NW-m1_Zachary_14"),
        "OBJECTIVE6 completes on the third stopped name"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_m01_writes_a_zero_mission_timer_and_one_space_help_labels() {
    let document = m01_control_document(&game_dir());
    let record = objective_record(&document);

    let mission_timer: Vec<&ZrdValue> = zrd_flat_fields(record)
        .into_iter()
        .filter(|(key, _)| *key == "MISSION_TIMER")
        .map(|(_, value)| value)
        .collect();
    assert_eq!(
        mission_timer,
        vec![&ZrdValue::List(vec![ZrdValue::Float(0.0)])],
        "M01 spells MISSION_TIMER as [0.0]; the measured start rule is value > 0.0f, so the \
         mission timer is never started for M01"
    );

    // The two SET_HELP_LABEL sites, verbatim: the second child is the label, and
    // both are a single space.
    let mut sites: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in zrd_flat_fields(record) {
        if !key.starts_with("OBJECTIVE")
            || !key["OBJECTIVE".len()..].bytes().all(|b| b.is_ascii_digit())
        {
            continue;
        }
        for (inner, inner_value) in zrd_directive_fields(value) {
            if inner != "SET_HELP_LABEL" {
                continue;
            }
            let children = inner_value.as_list().expect("a directive value is a list");
            let label = children
                .get(1)
                .and_then(ZrdValue::as_text)
                .expect("the label child");
            sites.push((
                key.to_owned(),
                children
                    .iter()
                    .map(|child| match child {
                        ZrdValue::Text(text) => text.clone(),
                        ZrdValue::List(names) => format!(
                            "[{}]",
                            names
                                .iter()
                                .map(|name| name.as_text().unwrap_or_default().to_owned())
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        other => format!("{other:?}"),
                    })
                    .collect(),
            ));
            assert_eq!(label, " ", "M01's measured SET_HELP_LABEL label");
        }
    }
    assert_eq!(
        sites,
        vec![
            (
                "OBJECTIVE2".to_owned(),
                vec!["[piratezep,rock_zeppelin]".to_owned(), " ".to_owned()]
            ),
            (
                "OBJECTIVE50".to_owned(),
                vec!["workersvoyagezep".to_owned(), " ".to_owned()]
            ),
        ],
        "the two SET_HELP_LABEL sites, as the document records them"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_the_music_groups_m01_spells_are_the_engines_seven_built_ins() {
    let document = m01_control_document(&game_dir());
    let (wakeup, _completed, _stopped) = m01_sound_groups(&document);

    // The seven names the executable's own table carries (0x63a4a8..0x63a540),
    // which is what makes a `*_SOUND_GROUP` naming one of them a music state
    // request rather than an ordinary cue.
    let built_in = [
        "music_battlesuccess_sg",
        "music_battle_sg",
        "music_missionsuccess_sg",
        "music_prebattle_sg",
        "music_primaryobj_sg",
        "music_secondaryobj_sg",
        "music_tertiaryobj_sg",
    ];
    let spelled: Vec<&String> = wakeup
        .values()
        .filter(|name| name.starts_with("music_"))
        .collect();
    assert_eq!(
        spelled.len(),
        6,
        "six of M01's 18 WAKEUP_SOUND_GROUP sites name a music group"
    );
    let mut distinct_music: Vec<&str> = spelled.iter().map(|name| name.as_str()).collect();
    distinct_music.sort_unstable();
    distinct_music.dedup();
    assert_eq!(
        distinct_music,
        vec![
            "music_battlesuccess_sg",
            "music_missionsuccess_sg",
            "music_prebattle_sg",
            "music_primaryobj_sg",
            "music_secondaryobj_sg",
        ],
        "five of the 13 distinct wakeup groups are music groups; music_prebattle_sg is spelled twice"
    );
    for name in &spelled {
        assert!(
            built_in.contains(&name.as_str()),
            "{name} is one of the engine's seven built-in music sound groups"
        );
    }
    // Every music-named block M01 spells is dormant, i.e. the cue fires on the
    // wake transition rather than at completion.
    for (block, name) in &wakeup {
        if name.starts_with("music_") {
            let dormant = measure_dormant_declarations(&document)
                .expect("every M01 block reads")
                .into_iter()
                .any(|measured| measured.block == *block && measured.begins_dormant());
            assert!(dormant, "{block} names {name} and begins dormant");
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_d_the_block_site_key_census_and_identity_classes_match_the_document() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");

    // "58 numbered objective blocks, 353 directive sites, 43 distinct keys" —
    // the census sentence the document measures M01's whole control record by.
    assert_eq!(
        (record.blocks(), record.sites(), record.vocabulary()),
        (58, 353, 43),
        "the document's block, site and vocabulary census of M01's control record"
    );

    // "M01's `IDENTITY` classes are `PRIMARY` (4 sites) and `SECONDARY` (1)" —
    // the classes whose completion sound the executable dispatches on, so M01's
    // class-sound claim is pinned with the record's own declarations.
    let mut roles: BTreeMap<String, u32> = BTreeMap::new();
    let mut declarations = 0;
    let document = m01_control_document(&game_dir());
    for block in measure_dormant_declarations(&document).expect("every M01 block reads") {
        for identity in &block.identities {
            *roles.entry(identity.role.clone()).or_default() += 1;
            declarations += 1;
        }
    }
    assert_eq!(declarations, 5, "M01 makes five IDENTITY declarations");
    assert_eq!(
        roles,
        BTreeMap::from([("PRIMARY".to_owned(), 4), ("SECONDARY".to_owned(), 1)]),
        "the document's IDENTITY classes: PRIMARY at 4 sites, SECONDARY at 1"
    );
}
