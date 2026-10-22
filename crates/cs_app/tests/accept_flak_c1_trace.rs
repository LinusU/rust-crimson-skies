//! Acceptance for Rally #1883 `FLAK-C1-TRACE`: the static trace of the flak
//! burst presentation and of `wep_27`'s weapon descriptor flag, recorded as
//! cited `[[flak_trace_fact]]` rows of `data/owner-static/engine-facts.toml`
//! (finding `docs/findings/2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`).
//!
//! The committed table is parsed by the production reader; the ignored tests
//! re-read every cited instruction from the owner's decrypted image and the
//! `wep_27` record from the owner's installation so a row cannot drift from
//! the bytes it cites.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

/// One `[[flak_trace_fact]]` row.
struct Row {
    name: String,
    va: usize,
    bytes: Vec<u8>,
    fact: String,
}

fn field(chunk: &str, name: &str) -> String {
    chunk
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("flak_trace_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[flak_trace_fact]]")
        .skip(1)
        .map(|chunk| {
            // A row ends at the next table header or the end of the file.
            let chunk = chunk.split("\n[").next().unwrap_or(chunk);
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-22");
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            let va = usize::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            Row {
                name: field(chunk, "name"),
                va,
                bytes,
                fact: field(chunk, "fact"),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no flak_trace_fact named {name}"))
}

#[test]
fn accept_flak_c1_the_committed_table_parses_with_the_trace_rows() {
    // The production reader refuses a table outside its strict subset; the
    // new rows must not break the launch's binding.
    let facts = engine_facts().expect("the committed table parses");
    assert_eq!(facts.image(), "crimson.decrypted.exe");
    let rows = rows();
    assert!(rows.len() >= 60, "only {} rows", rows.len());
    let mut names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), rows.len(), "row names are unique");
    for row in &rows {
        assert!(!row.bytes.is_empty(), "{} cites no bytes", row.name);
        assert!(!row.fact.is_empty(), "{} states no fact", row.name);
        let offset = facts
            .image_offset(row.va as u64)
            .unwrap_or_else(|error| panic!("{}: {error}", row.name));
        assert!(
            offset < 0x30_0000,
            "{} va {:#x} is past the image",
            row.name,
            row.va
        );
    }
}

#[test]
fn accept_flak_c1_the_rows_state_each_traced_law() {
    let rows = rows();
    // (a) the descriptor flag: CANNON sets 0x40, nothing else does.
    let flag = row(&rows, "descr.cannon_flag");
    assert_eq!(flag.va, 0x4ba9b6);
    assert_eq!(&flag.bytes[4..], [0x83, 0x0e, 0x40], "or dword [esi], 0x40");
    assert_eq!(row(&rows, "descr.cannon_string").bytes, b"CANNON\0");
    assert!(flag.fact.contains("CANNON key"));
    assert_eq!(row(&rows, "descr.ctor_clear").va, 0x5ae0ed);
    assert!(row(&rows, "descr.alloc").fact.contains("calloc(1, 0x38)"));
    // How a LOCK_ON rocket gets its target without the flag.
    for name in [
        "turret.target_node_store",
        "turret.lock_globals",
        "turret.lock_default",
        "rocket.lock_copy_gate",
        "rocket.lock_copy",
    ] {
        row(&rows, name);
    }
    assert!(
        row(&rows, "rocket.lock_copy_gate")
            .fact
            .contains("does not look at descriptor flag 0x40")
    );
    // (c) the range end.
    assert_eq!(row(&rows, "rocket.range_end_lock_expires").va, 0x5b01f9);
    assert_eq!(row(&rows, "weapon.expires_flag").va, 0x5adca3);
    assert!(
        row(&rows, "rocket.silent_end")
            .fact
            .contains("no burst call")
    );
    // (b) the burst presentation and the IF PLAYER_RANGE gate.
    for name in [
        "burst.impact_call",
        "burst.row_index",
        "burst.anim_guard",
        "burst.anim_start",
        "surface.table",
        "if.flag",
        "if.store_squared",
        "if.eval_compare",
        "if.player_pos_setter",
    ] {
        row(&rows, name);
    }
    assert_eq!(row(&rows, "burst.impact_call").bytes[0], 0xe8, "a call");
    assert_eq!(
        row(&rows, "if.player_range_string").bytes,
        b"PLAYER_RANGE\0"
    );
}

fn image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    std::fs::read(path).expect("read the engine image")
}

fn cstr_at(image: &[u8], va: usize) -> &[u8] {
    let start = va - IMAGE_BASE;
    let len = image[start..].iter().position(|&b| b == 0).unwrap();
    &image[start..start + len]
}

fn word_at(image: &[u8], va: usize) -> usize {
    let at = va - IMAGE_BASE;
    u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as usize
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_flak_c1_cited_rows_match_the_engine_image() {
    let image = image();
    for row in rows() {
        let offset = row.va - IMAGE_BASE;
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
    }
    // The strings the key tests push are the keys the rows name.
    let pushed = |name: &str| {
        let r = rows();
        let bytes = &row(&r, name).bytes;
        assert_eq!(bytes[0], 0x68, "{name}: push imm32");
        u32::from_le_bytes(bytes[1..5].try_into().unwrap()) as usize
    };
    assert_eq!(cstr_at(&image, pushed("descr.cannon_key")), b"CANNON");
    assert_eq!(cstr_at(&image, pushed("burst.tangler_key")), b"TANGLER");
    assert_eq!(cstr_at(&image, pushed("weapon.expires_key")), b"EXPIRES");
    assert_eq!(
        cstr_at(&image, pushed("if.player_range_key")),
        b"PLAYER_RANGE"
    );
    // The surface table: six names, index 0 is `default`.
    assert_eq!(word_at(&image, 0x637b10), 6);
    let names: Vec<&[u8]> = (0..6)
        .map(|i| cstr_at(&image, word_at(&image, 0x637b14 + 4 * i)))
        .collect();
    assert_eq!(
        names,
        [
            &b"default"[..],
            b"water",
            b"seafloor",
            b"quicksand",
            b"lava",
            b"fire"
        ]
    );
    // The setter stores its two arguments in the lock globals.
    let r = rows();
    let setter = &row(&r, "lock.globals_setter").bytes;
    let contains = |word: u32| setter.windows(4).any(|w| w == word.to_le_bytes());
    assert!(contains(0x00a1_d7cc) && contains(0x00a1_d7d0));
}

/// The bytes of a `weapons.zrd` record `wep_NN` in `ZBD/zrdr.zbd`: from the
/// record's name token to the next record's.
fn weapon_record(zbd: &[u8], number: u32, next: u32) -> &[u8] {
    let token = |text: String| {
        let mut bytes = 3u32.to_le_bytes().to_vec();
        bytes.extend((text.len() as u32).to_le_bytes());
        bytes.extend(text.as_bytes());
        bytes
    };
    let find = |haystack: &[u8], needle: &[u8], from: usize| {
        haystack[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|at| at + from)
    };
    // The definition (not a loadout mention) is followed by its DESC key.
    let mut definition = token(format!("wep_{number}"));
    definition.extend(4u32.to_le_bytes());
    let mut at = 0;
    let start = loop {
        let found = find(zbd, &definition, at).expect("the weapon definition");
        let after = found + definition.len() + 4;
        if zbd[after..].starts_with(&token("DESC".into())) {
            break found;
        }
        at = found + 1;
    };
    let end = find(zbd, &token(format!("wep_{next}")), start + 1).expect("the next record");
    &zbd[start..end]
}

fn has_key(record: &[u8], key: &str) -> bool {
    let mut token = 3u32.to_le_bytes().to_vec();
    token.extend((key.len() as u32).to_le_bytes());
    token.extend(key.as_bytes());
    record.windows(token.len()).any(|w| w == token)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flak_c1_wep_27_has_no_cannon_expires_or_detonate_at_range() {
    let dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the installation");
    let zbd = std::fs::read(std::path::Path::new(&dir).join("ZBD/zrdr.zbd")).expect("zrdr.zbd");
    let flak = weapon_record(&zbd, 27, 28);
    assert!(!has_key(flak, "CANNON"), "descriptor flag 0x40 stays clear");
    assert!(!has_key(flak, "EXPIRES"), "weapon flag 0x80 stays clear");
    assert!(!has_key(flak, "DETONATE_AT_RANGE"));
    assert!(!has_key(flak, "TANGLER"), "no impact hook");
    for key in [
        "LOCK_ON",
        "DETONATION_DISTANCE",
        "IMPACT_PROXIMITY",
        "CRATER",
    ] {
        assert!(has_key(flak, key), "wep_27 carries {key}");
    }
    assert!(has_key(flak, "flak_effect"), "the default IMPACT row");
    // The turret gun is the contrast: it is a CANNON.
    let gun = weapon_record(&zbd, 29, 30);
    assert!(has_key(gun, "CANNON"));
    // The compiled flak_effect carries PLAYER_RANGE squared (100 * 100).
    let anim = std::fs::read(std::path::Path::new(&dir).join("ZBD/C1/cam_anim.zbd"))
        .expect("C1/cam_anim.zbd");
    let name = anim
        .windows(11)
        .position(|w| w == b"flak_effect")
        .expect("flak_effect in the compiled animations");
    let floats: Vec<f32> = anim[name..name + 2400]
        .windows(4)
        .map(|w| f32::from_le_bytes(w.try_into().unwrap()))
        .collect();
    assert_eq!(floats.iter().filter(|&&f| f == 10000.0).count(), 1);
    assert!(!floats.contains(&100.0));
}
