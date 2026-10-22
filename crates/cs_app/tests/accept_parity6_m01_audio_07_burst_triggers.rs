//! Acceptance for Rally #1919 `PARITY6-M01-AUDIO-TIMELINE.07`: the call sites of
//! the engine's sound play helpers `0x593590` (2D) and `0x593b80` (3D), their
//! classification by event, and the map from each M01 burst buffer size to a
//! retail member (finding
//! `docs/findings/2026-10-22-m01-audio-burst-triggers.md`).
//!
//! The committed table is parsed by the production reader; the ignored test
//! re-reads the owner's decrypted image: every direct call of either helper in
//! `.text` must be one `[[audio_site]]` row (no site missing, none invented),
//! and every cited byte of the new `[[audio_fact]]` rows must match.
//! Task test prefix: `accept_parity6_m01_audio_07_`.

use std::collections::BTreeSet;

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;
const PLAY_2D: usize = 0x59_3590;
const PLAY_3D: usize = 0x59_3b80;
/// `.text` of the decrypted image: virtual address `0x401000`, `0x2014bd` bytes.
const TEXT: std::ops::Range<usize> = 0x40_1000..0x60_24bd;

/// The `[[audio_fact]]` rows this task added.
const NEW_FACT_ROWS: [&str; 21] = [
    "play.helper_dispatch",
    "play.weapon_fire",
    "play.weapon_row_pick_twin",
    "play.weapon_row_pick_0x5ad039",
    "play.weapon_row_pick_0x5b1387",
    "play.weapon_row_pick_0x5b0b64",
    "play.sequence_sound_event_3d",
    "play.sequence_sound_event_second",
    "play.player_bullet_hit",
    "config.warning_shot_sound",
    "config.bullet_hit_sound",
    "string.warning_shot_sound",
    "string.bullet_hit_sound",
    "config.touchdown_names",
    "string.touchdown_prefix",
    "play.dangerzone_camera",
    "play.select_weapon",
    "play.switch_target",
    "play.no_ammo_warning",
    "play.zep_destroyed",
    "play.music_state",
];

/// The event classes a site may carry.
const EVENTS: [&str; 17] = [
    "gun_shot",
    "bullet_impact",
    "warning_shot",
    "sequence_sound",
    "weapon_row_pick",
    "sound_node",
    "aircraft_voice",
    "music",
    "dispatcher",
    "danger_zone_cue",
    "mission_cue",
    "multiplayer_cue",
    "ui_cue",
    "unknown",
    "explosion",
    "ground_contact",
    "vo_radio",
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

fn sites() -> Vec<Section> {
    sections("audio_site")
}

fn facts() -> Vec<Section> {
    sections("audio_fact")
}

#[test]
fn accept_parity6_m01_audio_07_the_site_table_parses_and_is_classified() {
    engine_facts().expect("the committed table parses");
    let sites = sites();
    assert_eq!(sites.len(), 59, "49 calls of 0x593590 and 10 of 0x593b80");
    let mut seen = BTreeSet::new();
    for site in &sites {
        let va = site.number("va");
        assert!(seen.insert(va), "site {va:#x} is listed twice");
        assert!(TEXT.contains(&va), "site {va:#x} is outside .text");
        assert!(matches!(site.number("target"), PLAY_2D | PLAY_3D));
        assert_eq!(site.get("claim"), "OWNER-STATIC-2026-10-22");
        assert_eq!(site.get("provenance"), "static");
        let event = site.get("event");
        assert!(EVENTS.contains(&event), "{va:#x}: unknown class {event}");
        assert!(!site.get("function").is_empty(), "{va:#x} names no caller");
        assert!(!site.get("group").is_empty(), "{va:#x} names no group");
        assert!(
            !site.get("evidence").is_empty(),
            "{va:#x} cites no evidence"
        );
    }
    // A caller nobody could classify is recorded unknown, not guessed: the
    // sites named by the burst map are the classified ones.
    let events: BTreeSet<&str> = sites.iter().map(|site| site.get("event")).collect();
    for needed in [
        "gun_shot",
        "bullet_impact",
        "warning_shot",
        "sequence_sound",
        "weapon_row_pick",
        "unknown",
    ] {
        assert!(events.contains(needed), "no site is classified {needed}");
    }
}

#[test]
fn accept_parity6_m01_audio_07_the_burst_map_names_a_member_only_where_it_is_unique() {
    let bursts = sections("audio_burst");
    let sizes: Vec<usize> = bursts.iter().map(|b| b.number("size")).collect();
    assert_eq!(
        sizes,
        [267_168, 22_264, 16_192, 18_216, 78_936, 220_616],
        "one row per M01 burst size"
    );
    for burst in &bursts {
        let members: Vec<&str> = burst.get("members").split(',').collect();
        let groups: Vec<&str> = burst.get("groups").split(',').collect();
        assert_eq!(members.len(), groups.len(), "one group per member");
        match burst.get("status") {
            "member" => assert_eq!(members.len(), 1, "a named member is one member"),
            "candidates" => assert!(members.len() > 1, "a candidate set has several members"),
            other => panic!("unknown status {other}"),
        }
        assert!(burst.get("residual").len() > 20, "a residual is stated");
        assert!(!burst.get("event").is_empty());
        assert!(!burst.get("excluded").is_empty());
        for group in &groups {
            assert!(group.starts_with("snd_"), "{group} is a sounds.zrd group");
        }
    }
    // The 220,616 B size stays a candidate set: the exp_plane members and the
    // exp_ground members are reached by different events.
    let big = bursts.iter().find(|b| b.number("size") == 220_616).unwrap();
    assert_eq!(big.get("status"), "candidates");
    for member in [
        "exp_plane_1.wav",
        "exp_plane_3.wav",
        "exp_ground_a.wav",
        "exp_ground_b.wav",
    ] {
        assert!(big.get("members").contains(member));
    }
    // The three ricochet sizes are members of the one bullet_hit_sg group the
    // player.zrd bullet_hit_sound key names.
    for (size, member) in [
        (22_264, "ricco4.wav"),
        (16_192, "ricco3.wav"),
        (18_216, "ricco2.wav"),
    ] {
        let burst = bursts.iter().find(|b| b.number("size") == size).unwrap();
        assert_eq!(burst.get("members"), member);
        assert!(burst.get("event").starts_with("bullet_hit_sg"));
    }
}

#[test]
fn accept_parity6_m01_audio_07_the_new_fact_rows_are_present_and_cited() {
    let facts = facts();
    for name in NEW_FACT_ROWS {
        let row = facts
            .iter()
            .find(|row| row.get("name") == name)
            .unwrap_or_else(|| panic!("no audio_fact named {name}"));
        assert!(!row.bytes().is_empty(), "{name} cites no bytes");
        assert!(!row.get("fact").is_empty(), "{name} states no fact");
        assert_eq!(row.get("provenance"), "static");
    }
    // The key names are the ASCII strings the rows say they are.
    let strings = [
        ("string.warning_shot_sound", &b"warning_shot_sound\0"[..]),
        ("string.bullet_hit_sound", &b"bullet_hit_sound\0"[..]),
        ("string.touchdown_prefix", &b"touchdown_\0"[..]),
    ];
    for (name, text) in strings {
        let row = facts.iter().find(|row| row.get("name") == name).unwrap();
        assert_eq!(row.bytes(), text, "{name}");
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity6_m01_audio_07_cited_rows_and_every_call_site_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");

    // Every cited byte of the new rows.
    for row in facts() {
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

    // Every direct call of either helper in `.text`, found by scanning for
    // `e8 rel32`, is exactly one site row with the same target, and the image
    // has no `jmp` to either helper.
    let mut found = BTreeSet::new();
    for va in TEXT.start..TEXT.end - 5 {
        let at = va - IMAGE_BASE;
        let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
        let target = (va as i64 + 5 + i64::from(rel)) as usize;
        if target == PLAY_2D || target == PLAY_3D {
            match image[at] {
                0xe8 => {
                    found.insert((va, target));
                }
                0xe9 => panic!("a jmp to {target:#x} at {va:#x}"),
                _ => {}
            }
        }
    }
    let listed: BTreeSet<(usize, usize)> = sites()
        .iter()
        .map(|site| (site.number("va"), site.number("target")))
        .collect();
    assert_eq!(found.len(), 59, "the image has 59 direct calls");
    assert_eq!(listed, found, "the site table is the image's call list");
}
