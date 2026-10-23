//! Acceptance for Rally #1940 `PARITY6-M01-AUDIO-BURST-EVENTS`: what starts
//! the `touchdown_*` animations and which sound each ground-contact and crash
//! animation plays (finding
//! `docs/findings/2026-10-23-m01-audio-touchdown-and-crash-events.md`).
//!
//! The committed rows are parsed by the production reader. The ignored image
//! test re-reads every cited byte and the whole-image claims (who calls the
//! contact response, who reads the touchdown vector, who reads the soil
//! table). The ignored retail test reads `soils.zrd`, `touchdown.zrd`, the
//! destruction files and the sound table through the production readers and
//! pins the animation -> sound -> decoded size chain.
//! Task test prefix: `accept_parity6_m01_audio_09_`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cs_app::engine_facts::engine_facts;
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};

use super::accept_parity6_m01_audio_01_sample_identity::{archive_rows, game_dir, sounds_zrd};

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;
const TEXT: std::ops::Range<usize> = 0x40_1000..0x60_24bd;
const CONTACT_RESPONSE: usize = 0x48_d2c0;
const ANIMATION_STARTER: usize = 0x4e_dc10;

/// The `[[audio_fact]]` rows this task added.
const NEW_FACT_ROWS: [&str; 13] = [
    "soil.engine_table",
    "soil.table_reset_to_six",
    "touchdown.vector_fill",
    "touchdown.play_lookup",
    "touchdown.play_call",
    "touchdown.player_gate",
    "touchdown.caller_resolver_a",
    "touchdown.caller_resolver_b",
    "soil.height_query_water",
    "soil.description_parse",
    "crash.player_prefix",
    "crash.player_loader",
    "crash.ai_prefix",
];

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
fn accept_parity6_m01_audio_09_the_rows_are_present_cited_and_say_what_the_finding_says() {
    engine_facts().expect("the committed table parses");
    for name in NEW_FACT_ROWS.iter().chain(&["crash.ai_loader"]) {
        let row = fact(name);
        assert_eq!(row.get("claim"), "OWNER-STATIC-2026-10-22", "{name}");
        assert_eq!(row.get("provenance"), "static", "{name}");
        assert!(!row.bytes().is_empty(), "{name} cites no bytes");
        assert!(row.get("fact").len() > 40, "{name} states no fact");
        assert!((0x40_1000..0x70_0000).contains(&row.number("va")), "{name}");
    }
    // The built-in soil names, in index order: `water` is index 1.
    let names: Vec<Vec<u8>> = fact("soil.engine_table")
        .bytes()
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    assert_eq!(
        names,
        [
            b"default".to_vec(),
            b"water".to_vec(),
            b"seafloor".to_vec(),
            b"quicksand".to_vec(),
            b"lava".to_vec(),
            b"fire".to_vec()
        ]
    );
    // The reset stores the immediate 6 into the soil count.
    assert_eq!(
        fact("soil.table_reset_to_six").bytes(),
        [0xc7, 0x05, 0x10, 0x7b, 0x63, 0x00, 0x06, 0, 0, 0]
    );
    assert_eq!(fact("crash.player_prefix").bytes(), b"player_crash_\0");
    assert_eq!(fact("crash.ai_prefix").bytes(), b"ai_crash_\0");
    // The impulse gate compares with the float at 0x6032c8, and each resolver
    // ends in the relative call of the contact response.
    for (name, call_at) in [
        ("touchdown.caller_resolver_a", 0x48_ed8busize),
        ("touchdown.caller_resolver_b", 0x49_0b08),
    ] {
        let row = fact(name);
        let bytes = row.bytes();
        let end = row.number("va") + bytes.len();
        let rel = i32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap());
        assert_eq!(
            (end as i64 + i64::from(rel)) as usize,
            CONTACT_RESPONSE,
            "{name}"
        );
        assert_eq!(end, call_at + 5, "{name}");
    }
    // The bursts keep their statuses: the contact is decoded, the event is not.
    let bursts = sections("audio_burst");
    let water = bursts.iter().find(|b| b.number("size") == 78_936).unwrap();
    assert!(water.get("event").contains("0x48d2c0"));
    assert!(water.get("residual").contains("unknown"));
    let big = bursts.iter().find(|b| b.number("size") == 220_616).unwrap();
    assert_eq!(big.get("status"), "candidates");
    assert!(big.get("residual").contains("unknown"));
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

/// Virtual addresses of every 32-bit little-endian `value` in the image.
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
fn accept_parity6_m01_audio_09_cited_rows_match_the_engine_image_and_the_callers_are_closed() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");

    for row in sections("audio_fact") {
        if !NEW_FACT_ROWS.contains(&row.get("name")) && row.get("name") != "crash.ai_loader" {
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

    // The contact response has exactly the two resolver callers.
    assert_eq!(
        callers(&image, CONTACT_RESPONSE),
        BTreeSet::from([0x48_ed8b, 0x49_0b08])
    );
    // The touchdown vector (begin 0x71c2e8) is built at mission start and read
    // at one place, the lookup inside the contact response.
    assert_eq!(dword_refs(&image, 0x71_c2e8), [0x47_3751, 0x48_d41b]);
    // The animation starter has 15 callers; only 0x48d4b9 is in the contact
    // response.
    let starters = callers(&image, ANIMATION_STARTER);
    assert_eq!(starters.len(), 15);
    assert!(starters.contains(&0x48_d4b9));
    // The soil table: the three accessors and the loader are the only code
    // that touches the count dword 0x637b10.
    let count_refs: BTreeSet<usize> = dword_refs(&image, 0x63_7b10)
        .into_iter()
        .filter(|va| TEXT.contains(va))
        .collect();
    assert_eq!(
        count_refs.len(),
        18,
        "readers and writers of the soil count: {count_refs:x?}"
    );
    // The file image holds the initial table: its six name pointers must reach
    // the cited names.
    let pointers: Vec<usize> = (0..6)
        .map(|i| {
            let at = 0x63_7b14 - IMAGE_BASE + 4 * i;
            u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as usize
        })
        .collect();
    let name_at = |va: usize| {
        let start = va - IMAGE_BASE;
        let len = image[start..].iter().position(|b| *b == 0).unwrap();
        String::from_utf8(image[start..start + len].to_vec()).unwrap()
    };
    let names: Vec<String> = pointers.iter().map(|va| name_at(*va)).collect();
    assert_eq!(
        names,
        ["default", "water", "seafloor", "quicksand", "lava", "fire"]
    );
    // `soil_` is parsed in the node-description parser and in the GameGen
    // record tokenizer, nowhere else.
    assert_eq!(dword_refs(&image, 0x62_bf98), [0x4c_3015, 0x4c_56d2]);
}

fn reader_member(root: &Path, name: &str) -> ZrdValue {
    let container = "ZBD/zrdr.zbd";
    let bytes = std::fs::read(root.join(container)).expect("zrdr.zbd readable");
    let spelling = cs_types::install::RelativePath::new(container).expect("relative path");
    let mut context = ParseContext::with_defaults("parity6_audio_09");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes)).expect("dispatch");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    let entry = archive
        .entries()
        .find(|e| String::from_utf8_lossy(e.name()).eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("{name} is not in zrdr.zbd"));
    decode_zrd(entry.content()).unwrap_or_else(|e| panic!("{name} decodes: {e:?}"))
}

fn first_text(value: &ZrdValue) -> Option<&str> {
    value
        .as_text()
        .or_else(|| value.as_list()?.first().and_then(first_text))
}

/// Every sound or called animation named in `node`, in document order.
fn collect(node: &ZrdValue, keyword: &str, out: &mut Vec<String>) {
    let Some(children) = node.as_list() else {
        return;
    };
    for (i, child) in children.iter().enumerate() {
        if child.as_text() == Some(keyword)
            && let Some(body) = children.get(i + 1).and_then(ZrdValue::as_list)
        {
            // `["NAME", [x]...]` carries the name in the second element.
            if let Some(name) = body.get(1).and_then(first_text) {
                out.push(name.to_owned());
            }
        }
        collect(child, keyword, out);
    }
}

/// `ANIMATION_NAME -> (sounds, called animations)` of every definition.
fn animations(node: &ZrdValue, out: &mut BTreeMap<String, (Vec<String>, Vec<String>)>) {
    let Some(children) = node.as_list() else {
        return;
    };
    for (i, child) in children.iter().enumerate() {
        if child.as_text() == Some("ANIMATION_DEFINITION")
            && let Some(body) = children.get(i + 1)
        {
            // ANIMATION_NAME's body is `[name]`, not `["NAME", ..]`.
            let list = body.as_list().expect("definition body");
            let anim = list
                .iter()
                .position(|c| c.as_text() == Some("ANIMATION_NAME"))
                .and_then(|p| list.get(p + 1))
                .and_then(first_text)
                .expect("ANIMATION_NAME");
            let mut sounds = Vec::new();
            collect(body, "SOUND", &mut sounds);
            let mut calls = Vec::new();
            collect(body, "CALL_ANIMATION", &mut calls);
            out.insert(anim.to_owned(), (sounds, calls));
        }
        animations(child, out);
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_09_retail_contact_and_crash_animations_play_the_sounds_the_finding_lists()
 {
    let root = game_dir();

    // soils.zrd names the engine-appended soils; none of them is `water`.
    let soils = reader_member(&root, "soils.zrd");
    let mut soil_names = Vec::new();
    fn texts(node: &ZrdValue, out: &mut Vec<String>) {
        if let Some(t) = node.as_text() {
            out.push(t.to_owned());
        } else if let Some(children) = node.as_list() {
            children.iter().for_each(|c| texts(c, out));
        }
    }
    texts(&soils, &mut soil_names);
    assert_eq!(
        soil_names,
        [
            "player",
            "enemy",
            "airstrip",
            "opensesame",
            "death",
            "buildings",
            "dzone",
            "dirt"
        ]
    );

    let mut touch = BTreeMap::new();
    animations(&reader_member(&root, "touchdown.zrd"), &mut touch);
    assert_eq!(
        touch.keys().cloned().collect::<Vec<_>>(),
        ["touchdown_default", "touchdown_dirt", "touchdown_water"]
    );
    assert_eq!(touch["touchdown_default"].0, strings(&["snd_exp_ground_b"]));
    assert_eq!(touch["touchdown_dirt"].0, strings(&["snd_exp_ground_b"]));
    assert_eq!(touch["touchdown_water"].0, strings(&["snd_exp_water_b"]));
    // touchdown_<soil> for the other soils (player, enemy, ...) is not defined,
    // so the lookup by name finds no animation for them.

    let mut crash = BTreeMap::new();
    animations(
        &reader_member(&root, "player_plane_destruct.zrd"),
        &mut crash,
    );
    animations(&reader_member(&root, "ai_plane_destruct.zrd"), &mut crash);
    assert_eq!(
        crash["player_crash_dirt"].0,
        strings(&["snd_exp_ground_a", "ground_mixed_exp_sg"])
    );
    assert_eq!(
        crash["player_crash_default"].0,
        strings(&["plane_destroy_sg", "air_mixed_exp_sg"])
    );
    assert!(crash["player_crash_water"].0.is_empty());
    assert!(
        crash["player_crash_water"]
            .1
            .contains(&"plane_big_splash".to_owned())
    );
    assert_eq!(crash["ai_crash_dirt"].0, strings(&["snd_exp_ground_a"]));
    assert_eq!(crash["ai_crash_default"].0, strings(&["air_mixed_exp_sg"]));
    assert!(crash["ai_crash_water"].0.is_empty());
    assert!(
        crash["ai_crash_water"]
            .1
            .contains(&"plane_big_splash".to_owned())
    );

    // plane_big_splash itself plays snd_exp_water_a, not snd_exp_water_b.
    let mut splash = BTreeMap::new();
    animations(&reader_member(&root, "plane_splash.zrd"), &mut splash);
    assert_eq!(splash["plane_big_splash"].0, strings(&["snd_exp_water_a"]));

    // The decoded sizes: the water crash (exp_water_a) and the other two
    // destruction members (exp_plane_2/4) are not burst sizes.
    let sizes: BTreeMap<String, u64> = archive_rows(&root, "ZBD/soundsh.zbd")
        .into_iter()
        .map(|row| (row.0, row.4))
        .collect();
    let zrd = sounds_zrd(&root);
    let mut files = BTreeMap::new();
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
        children.iter().for_each(|c| sound_files(c, out));
    }
    sound_files(&zrd, &mut files);
    let size_of = |sound: &str| sizes[&files[sound]];
    assert_eq!(size_of("snd_exp_water_b"), 78_936);
    assert_eq!(size_of("snd_exp_water_a"), 151_800);
    assert_eq!(size_of("snd_exp_ground_b"), 220_616);
    assert_eq!(size_of("snd_exp_ground_a"), 220_616);
    assert_eq!(size_of("snd_exp_plane1"), 220_616);
    assert_eq!(size_of("snd_exp_plane3"), 220_616);
    assert_eq!(size_of("snd_exp_plane2"), 242_880);
    assert_eq!(size_of("snd_exp_plane4"), 259_072);
}
