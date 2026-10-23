//! Acceptance for Rally #1939 `PARITY6-M01-AUDIO-SFX-PICK`: how the engine
//! picks one member from a `DYNAMIC_WEIGHTS` sound group, and which `VO_*`
//! lines that share an M01 burst size the mission can start (finding
//! `docs/findings/2026-10-23-m01-audio-sfx-group-pick.md`).
//!
//! The committed rows are parsed by the production reader. The ignored image
//! test re-reads every cited byte of the new `[[audio_fact]]` rows and checks
//! the claims that are about the whole image (the selector has two callers;
//! the keyword is parsed in one place). The ignored retail test reads
//! `sounds.zrd` and M01's objective record through the production readers.
//! Task test prefix: `accept_parity6_m01_audio_08_`.

use std::collections::{BTreeMap, BTreeSet};

use cs_app::engine_facts::engine_facts;
use cs_app::mission_session::{declared_engine_objectives, declared_sound_groups};
use cs_app::objectives::recover_retail_objectives;
use cs_content::stunts::ZrdValue;

use super::accept_parity6_m01_audio_01_sample_identity::{archive_rows, game_dir, sounds_zrd};

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;
const TEXT: std::ops::Range<usize> = 0x40_1000..0x60_24bd;
const SELECTOR: usize = 0x59_a440;

/// The `[[audio_fact]]` rows this task added.
const NEW_FACT_ROWS: [&str; 27] = [
    "group.handle_kind",
    "group.lookup_by_name",
    "group.play_dispatch",
    "group.voice_start",
    "group.voice_init_pick",
    "group.voice_init_call_selector",
    "group.voice_state_table",
    "group.voice_repick",
    "group.voice_repeat_count",
    "group.selector_single_member",
    "group.selector_total",
    "group.selector_draw",
    "group.selector_walk",
    "group.selector_pick_store",
    "group.selector_floor",
    "group.selector_normalise",
    "group.selector_constants",
    "group.selector_constant_inv32767",
    "group.selector_constant_100",
    "group.dynamic_weights_parse",
    "group.default_weights",
    "group.member_play_count_default",
    "group.weight_keyword",
    "group.rand_seed",
    "pilot.voice_name_random",
    "pilot.voice_name_fallback",
    "pilot.voice_cue_allydown",
];

/// The decoded sizes of the six M01 burst buffers.
const BURST_SIZES: [u64; 6] = [267_168, 22_264, 16_192, 18_216, 78_936, 220_616];

/// One `key = value` section of the table (a `[[array]]` element).
struct Section {
    fields: Vec<(String, String)>,
}

impl Section {
    fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("section lacks {key}"))
    }

    fn number(&self, key: &str) -> usize {
        let text = self.get(key);
        match text.strip_prefix("0x") {
            Some(hex) => usize::from_str_radix(hex, 16).expect("hex number"),
            None => text.parse().expect("decimal number"),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let hex = self.get("bytes");
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
            .collect()
    }
}

fn sections(array: &str) -> Vec<Section> {
    let header = format!("[[{array}]]");
    let mut out = Vec::new();
    let mut current: Option<Section> = None;
    for line in TABLE.lines() {
        if line.starts_with('[') {
            out.extend(current.take());
            if line.trim() == header {
                current = Some(Section { fields: Vec::new() });
            }
        } else if let Some(section) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            section.fields.push((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }
    }
    out.extend(current);
    out
}

fn fact(name: &str) -> Section {
    sections("audio_fact")
        .into_iter()
        .find(|row| row.get("name") == name)
        .unwrap_or_else(|| panic!("no audio_fact named {name}"))
}

#[test]
fn accept_parity6_m01_audio_08_the_new_fact_rows_are_present_and_cited() {
    engine_facts().expect("the committed table parses");
    for name in NEW_FACT_ROWS {
        let row = fact(name);
        assert_eq!(row.get("claim"), "OWNER-STATIC-2026-10-22", "{name}");
        assert_eq!(row.get("provenance"), "static", "{name}");
        assert!(!row.bytes().is_empty(), "{name} cites no bytes");
        assert!(row.get("fact").len() > 20, "{name} states no fact");
        let va = row.number("va");
        assert!((0x40_1000..0x70_0000).contains(&va), "{name}: {va:#x}");
    }
    // The constants and strings are what the rows say they are.
    assert_eq!(
        f32::from_le_bytes(fact("group.selector_constants").bytes().try_into().unwrap()),
        0.001
    );
    assert_eq!(
        f32::from_le_bytes(
            fact("group.selector_constant_inv32767")
                .bytes()
                .try_into()
                .unwrap()
        ),
        1.0 / 32767.0
    );
    assert_eq!(
        f32::from_le_bytes(
            fact("group.selector_constant_100")
                .bytes()
                .try_into()
                .unwrap()
        ),
        100.0
    );
    assert_eq!(
        fact("pilot.voice_name_random").bytes(),
        b"snd_%s_id%d_random\0"
    );
    assert_eq!(fact("pilot.voice_name_fallback").bytes(), b"snd_id%d_%s\0");
}

#[test]
fn accept_parity6_m01_audio_08_vo_lines_of_a_burst_size_are_excluded_except_one() {
    let lines = sections("audio_vo_line");
    assert_eq!(lines.len(), 9, "one row per VO line that shares a size");
    let mut reachable = Vec::new();
    for line in &lines {
        assert_eq!(line.get("claim"), "OWNER-STATIC-2026-10-22");
        assert_eq!(line.get("provenance"), "static");
        assert!(BURST_SIZES.contains(&(line.number("size") as u64)));
        assert!(line.get("wav").starts_with("VO_"));
        assert!(line.get("sound").starts_with("snd_"));
        assert!(line.get("reason").len() > 20);
        match line.get("status") {
            "excluded" => {}
            "reachable" => reachable.push(line.get("wav").to_owned()),
            other => panic!("unknown status {other}"),
        }
    }
    // Both lines of the 267,168 B size are excluded; the 78,936 B size keeps
    // exactly one engine-reachable line.
    for line in lines.iter().filter(|l| l.number("size") == 267_168) {
        assert_eq!(line.get("status"), "excluded", "{}", line.get("wav"));
    }
    assert_eq!(reachable, ["VO_id1_GL-AllyDwn-B.wav"]);
    assert_eq!(
        lines
            .iter()
            .find(|l| l.get("status") == "reachable")
            .map(|l| l.number("size")),
        Some(78_936)
    );
}

/// Callers (`e8 rel32`) of `target` in `.text`.
fn callers(image: &[u8], target: usize) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    for va in TEXT.start..TEXT.end - 5 {
        let at = va - IMAGE_BASE;
        if image[at] != 0xe8 {
            continue;
        }
        let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
        if (va as i64 + 5 + i64::from(rel)) as usize == target {
            found.insert(va);
        }
    }
    found
}

/// Virtual addresses of every aligned-or-not 32-bit little-endian `value`.
fn dword_refs(image: &[u8], value: usize) -> Vec<usize> {
    let needle = (value as u32).to_le_bytes();
    image
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(at, _)| at + IMAGE_BASE)
        .collect()
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity6_m01_audio_08_cited_rows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");

    for row in sections("audio_fact") {
        if !NEW_FACT_ROWS.contains(&row.get("name")) {
            continue;
        }
        let offset = row.number("va") - IMAGE_BASE;
        let bytes = row.bytes();
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{} at {:#x}",
            row.get("name"),
            row.number("va")
        );
    }

    // The selector has exactly two callers: the voice start (state 0) and the
    // repeat (state 2). One play of a group is one pick.
    assert_eq!(
        callers(&image, SELECTOR),
        BTreeSet::from([0x59_a40f, 0x59_a846]),
        "callers of the member selector 0x59a440"
    );
    // The play helper forwards a group handle to the voice start: the only
    // caller of 0x59ab60 are the two thin wrappers 0x59ab40 and 0x59ad20.
    assert_eq!(
        callers(&image, 0x59_ab60),
        BTreeSet::from([0x59_ab50, 0x59_ad36])
    );
    // The DYNAMIC_WEIGHTS keyword string is referenced from the group parser
    // only, so no sound-effect-specific parser reads it.
    assert_eq!(dword_refs(&image, 0x63_af8c), [0x59_9cf0]);
    // `rand` is the import slot the selector calls, and the selector's draw is
    // the only use of the 1/32768 scale constant inside the selector.
    let draw = fact("group.selector_draw").bytes();
    assert_eq!(&draw[11..17], &[0xff, 0x15, 0x50, 0x03, 0xa2, 0x00]);
}

/// Text of a `ZrdValue::Text`, if it is one.
fn text(value: &ZrdValue) -> Option<&str> {
    value.as_text()
}

/// Every `[name "DYNAMIC_WEIGHTS" factor member...]` record of the document.
fn dynamic_groups<'a>(node: &'a ZrdValue, out: &mut Vec<&'a [ZrdValue]>) {
    let Some(children) = node.as_list() else {
        return;
    };
    if children.len() > 2 && text(&children[1]) == Some("DYNAMIC_WEIGHTS") {
        out.push(children);
        return;
    }
    for child in children {
        dynamic_groups(child, out);
    }
}

/// `(sound name, wav)` of every sound record, and the child group records
/// reachable by name, to expand a group to its files.
fn sound_files(node: &ZrdValue, out: &mut BTreeMap<String, String>) {
    let Some(children) = node.as_list() else {
        return;
    };
    if let (Some(name), Some(file)) = (
        children.first().and_then(ZrdValue::as_text),
        children.get(1).and_then(ZrdValue::as_text),
    ) && file.to_ascii_lowercase().ends_with(".wav")
    {
        out.entry(name.to_owned())
            .or_insert_with(|| file.to_owned());
        return;
    }
    for child in children {
        sound_files(child, out);
    }
}

/// Collect every `snd_*` name in a group record, depth first.
fn names_in(node: &ZrdValue, out: &mut Vec<String>) {
    match node {
        ZrdValue::List(children) => children.iter().for_each(|c| names_in(c, out)),
        ZrdValue::Text(t) if t.starts_with("snd_") => out.push(t.clone()),
        _ => {}
    }
}

/// The `SOUND_GROUPS` records that are not single sounds, by group name.
fn group_records<'a>(node: &'a ZrdValue, out: &mut BTreeMap<String, &'a ZrdValue>) {
    let Some(children) = node.as_list() else {
        return;
    };
    let is_sound = children
        .get(1)
        .and_then(ZrdValue::as_text)
        .is_some_and(|f| f.to_ascii_lowercase().ends_with(".wav"));
    if !is_sound
        && let Some(name) = children.first().and_then(ZrdValue::as_text)
        && children[1..].iter().any(|c| c.as_list().is_some())
    {
        out.entry(name.to_owned()).or_insert(node);
    }
    for child in children {
        group_records(child, out);
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_08_retail_groups_start_equal_and_m01_names_no_burst_size_vo() {
    let root = game_dir();
    let zrd = sounds_zrd(&root);

    // Every DYNAMIC_WEIGHTS record of sounds.zrd: the factor is 0.5 (two
    // spell 0.2), and no member list spells WEIGHT, PLAY_COUNT, REPEAT or any
    // other keyword, so the selector's members all start at 100 / count.
    let mut groups = Vec::new();
    dynamic_groups(&zrd, &mut groups);
    assert_eq!(groups.len(), 472, "DYNAMIC_WEIGHTS records in sounds.zrd");
    let mut factors = BTreeMap::new();
    for group in &groups {
        let factor = match &group[2] {
            ZrdValue::Float(value) => *value,
            other => panic!("{:?}: factor {other:?}", group[0]),
        };
        *factors.entry(factor.to_bits()).or_insert(0usize) += 1;
        for member in &group[3..] {
            let list = member.as_list().expect("a member is a list");
            assert!(
                list.iter().all(
                    |c| text(c).is_some_and(|t| t.starts_with("snd_")) || c.as_list().is_some()
                ),
                "{:?}: a member spells a keyword",
                group[0]
            );
        }
    }
    assert_eq!(
        factors,
        BTreeMap::from([(0.5f32.to_bits(), 470), (0.2f32.to_bits(), 2)])
    );

    // The groups the M01 burst needs, in the table's member order.
    let mut by_name = BTreeMap::new();
    for group in &groups {
        let mut names = Vec::new();
        for member in &group[3..] {
            names_in(member, &mut names);
        }
        by_name.insert(text(&group[0]).unwrap().to_owned(), names);
    }
    let expect: [(&str, &[&str]); 7] = [
        (
            "bullet_hit_sg",
            &[
                "snd_ricochet1",
                "snd_ricochet2",
                "snd_ricochet3",
                "snd_ricochet4",
            ],
        ),
        (
            "air_mixed_exp_sg",
            &[
                "snd_exp_hit1",
                "snd_exp_hit2",
                "snd_exp_hit3",
                "snd_exp_hit3a",
                "snd_exp_hit5",
            ],
        ),
        (
            "ground_mixed_exp_sg",
            &[
                "snd_exp_hit1",
                "snd_exp_hit1g",
                "snd_exp_hit2",
                "snd_exp_hit3",
                "snd_exp_hit3g",
            ],
        ),
        (
            "plane_destroy_sg",
            &[
                "snd_exp_plane1",
                "snd_exp_plane2",
                "snd_exp_plane3",
                "snd_exp_plane4",
            ],
        ),
        (
            "bullet_warning_sg",
            &["snd_bulletpass1", "snd_bulletpass2", "snd_bulletpass3"],
        ),
        (
            "window_hit_sg",
            &["snd_windowhit1", "snd_windowhit2", "snd_windowhit3"],
        ),
        (
            "snd_GL-AllyDwn-A_id1_random",
            &[
                "snd_id1_GL-AllyDwn-A",
                "snd_id1_GL-AllyDwn-B",
                "snd_id1_GL-AllyDwn-C",
            ],
        ),
    ];
    for (group, members) in expect {
        assert_eq!(
            by_name.get(group).map(Vec::as_slice),
            Some(
                members
                    .iter()
                    .map(|m| (*m).to_owned())
                    .collect::<Vec<_>>()
                    .as_slice()
            ),
            "{group}"
        );
    }

    // M01 (`ZBD/C3/M01`, "The Lost Treasure"): expand every sound group its
    // objective record cues to files; none decodes to a burst size.
    let recovery = recover_retail_objectives(&root, "zbd/c3/m01").expect("M01 reads");
    let engine = declared_engine_objectives(&recovery).expect("M01 lifecycle model");
    let declared = declared_sound_groups(&engine);
    assert_eq!(declared.len(), 25, "M01 cues 25 sound groups");

    let mut files = BTreeMap::new();
    sound_files(&zrd, &mut files);
    let mut records = BTreeMap::new();
    group_records(&zrd, &mut records);
    let sizes: BTreeMap<String, u64> = archive_rows(&root, "ZBD/soundsh.zbd")
        .into_iter()
        .map(|row| (row.0, row.4))
        .collect();
    let mut reached = 0usize;
    for group in &declared {
        let mut names = Vec::new();
        match records.get(group.as_str()) {
            Some(record) => names_in(record, &mut names),
            None => names.push(group.clone()),
        }
        for name in names {
            let Some(file) = files.get(&name) else {
                continue;
            };
            reached += 1;
            let size = sizes
                .get(file)
                .unwrap_or_else(|| panic!("{file} is not in soundsh.zbd"));
            assert!(
                !BURST_SIZES.contains(size),
                "M01 group {group} reaches {file} of burst size {size}"
            );
            assert!(
                file.starts_with("VO_c1-HA-m1_") || file.starts_with("music_"),
                "{group} reaches {file}, neither an M01 line nor a music track"
            );
        }
    }
    assert!(reached >= declared.len(), "every group reaches a file");
}
