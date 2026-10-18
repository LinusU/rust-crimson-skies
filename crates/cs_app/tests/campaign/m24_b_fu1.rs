//! Acceptance stage M24-B-FU1: the operand declarations M24's control member
//! spells, audited against the whole installation.
//!
//! M24-B (`m24_b.rs`) cross-checked only the three zeppelin names, the five
//! `M4*` nets and the five `dzpath` zones. This follow-up applies the M17-B
//! method to every other text the control member
//! (`ZBD/C5/M04/zrdr.zbd#objectives.zrd`) spells: one `.zrd` text index of the
//! installation built through production discovery
//! (`cs_formats::script_raw::discover_container`) and the production decoder
//! (`cs_content::stunts::decode_zrd`), then a raw-byte scan of every file for
//! each text that index does not declare outside the control member.
//!
//! An undeclared name is recorded as a gap (`docs/findings/
//! 2026-10-18-m24-b-fu1-operand-declarations.md`); what the original resolves
//! it to is not guessed. The retail tests are `#[ignore = "requires
//! CS_GAME_DIR"]`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_assets::install::discover;
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

/// The reader archive the installation ships for M24.
const CONTAINER: &str = "ZBD/C5/M04/zrdr.zbd";
/// The control member.
const CONTROL_MEMBER: &str = "objectives.zrd";

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M24-B-FU1 needs the retail capability; run this suite \
             with `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// Collects every text node of a decoded `.zrd` document.
fn walk_texts(value: &ZrdValue, texts: &mut Vec<String>) {
    match value {
        ZrdValue::Text(text) => texts.push(text.clone()),
        ZrdValue::List(children) => {
            for child in children {
                walk_texts(child, texts);
            }
        }
        _ => {}
    }
}

/// Every `.zrd` text the installation declares: the text to the set of
/// `(container, member)` pairs that spell it, through production discovery.
fn text_index() -> &'static BTreeMap<String, BTreeSet<(String, String)>> {
    static INDEX: OnceLock<BTreeMap<String, BTreeSet<(String, String)>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let found = discover(&game_dir()).expect("production discovery reads the installation");
        let mut index: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        for record in &found.manifest.files {
            let spelling = record.relative_spelling.as_str();
            if !spelling.to_lowercase().ends_with(".zbd") {
                continue;
            }
            let Ok(relative) = RelativePath::new(&spelling.to_lowercase()) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(found.manifest.host_root.join(spelling)) else {
                continue;
            };
            let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
            for program in discovery.programs() {
                let Some(member) = program.locator().member() else {
                    continue;
                };
                let Ok(document) = decode_zrd(program.bytes()) else {
                    continue;
                };
                let mut texts = Vec::new();
                walk_texts(&document, &mut texts);
                for text in texts {
                    index
                        .entry(text)
                        .or_default()
                        .insert((spelling.to_owned(), member.to_owned()));
                }
            }
        }
        index
    })
}

/// Whether `(container, member)` is M24's control member.
fn is_control(container: &str, member: &str) -> bool {
    container.eq_ignore_ascii_case(CONTAINER) && member == CONTROL_MEMBER
}

/// The distinct texts of M24's control member, decoded fresh from the archive.
fn control_texts() -> Vec<String> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| {
            record
                .relative_spelling
                .as_str()
                .eq_ignore_ascii_case(CONTAINER)
        })
        .expect("M24's reader archive is in the installation");
    let relative = RelativePath::new(&CONTAINER.to_lowercase()).expect("relative path");
    let bytes = std::fs::read(
        found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str()),
    )
    .expect("the archive reads");
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    let mut texts = Vec::new();
    for program in discovery.programs() {
        if program.locator().member() != Some(CONTROL_MEMBER) {
            continue;
        }
        let document = decode_zrd(program.bytes()).expect("the control member decodes");
        walk_texts(&document, &mut texts);
    }
    texts.sort();
    texts.dedup();
    texts
}

/// Whether `text` is a directive key or block address the record's own grammar
/// spells: upper-case ASCII, digits and underscores only.
fn is_keyword(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The members, other than M24's control member, that spell `text`.
fn declarations_of(text: &str) -> Vec<(String, String)> {
    text_index()
        .get(text)
        .map(|hits| {
            hits.iter()
                .filter(|(container, member)| !is_control(container, member))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `(container, member)` is spelled by `text` in the given member of
/// the given container.
fn declared_in(text: &str, container: &str, member: &str) -> bool {
    declarations_of(text)
        .iter()
        .any(|(c, m)| c.eq_ignore_ascii_case(container) && m == member)
}

/// The installation files whose raw bytes contain `needle`, with the count of
/// occurrences — the scan that backs "declared by no file", independent of
/// any decoder.
fn raw_occurrences(needle: &str) -> Vec<(String, usize)> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits = Vec::new();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        let Ok(bytes) = std::fs::read(found.manifest.host_root.join(spelling)) else {
            continue;
        };
        let count = bytes
            .windows(needle.len())
            .filter(|window| *window == needle.as_bytes())
            .count();
        if count > 0 {
            hits.push((spelling.to_owned(), count));
        }
    }
    hits.sort();
    hits
}

/// **Of the 226 distinct texts M24's record spells, exactly three are declared
/// by no other `.zrd` member of the installation.**
///
/// They are `SET_AI_` — the truncated key M24-B already recorded — the aircraft
/// `stihellhound_5_eg0`, and `MSG_BRF_NYM4_OBJ1`, which is a string-table
/// identifier in `strings.dll` (see the last test).
/// Every other text is declared by some other member; the texts that only
/// *another mission's control member* spells are directive keys, block
/// addresses and two sound names that `sounds.zrd` declares as well.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_fu1_the_texts_no_other_member_declares_are_pinned_exactly() {
    let texts = control_texts();
    assert_eq!(
        texts.len(),
        226,
        "the control member's distinct texts changed"
    );

    let undeclared: Vec<&str> = texts
        .iter()
        .map(String::as_str)
        .filter(|text| declarations_of(text).is_empty())
        .collect();
    assert_eq!(
        undeclared,
        ["MSG_BRF_NYM4_OBJ1", "SET_AI_", "stihellhound_5_eg0"],
        "the set of texts no other .zrd member declares changed"
    );

    // Texts whose only other spellings are other missions' control members
    // are not declarations either; they must all be grammar words.
    let control_only: Vec<&str> = texts
        .iter()
        .map(String::as_str)
        .filter(|text| {
            let others = declarations_of(text);
            !others.is_empty() && others.iter().all(|(_, member)| member == CONTROL_MEMBER)
        })
        .filter(|text| !is_keyword(text) && !text.starts_with("music_"))
        .collect();
    assert_eq!(
        control_only,
        Vec::<&str>::new(),
        "an operand is spelled only by other control members"
    );
    // The two Sparks sound names look like that case but `sounds.zrd` declares
    // them, so they are not control-only.
    for name in ["snd_c3-HW-m4_Sparks_33", "snd_c5-MN-m4_Sparks_175"] {
        assert!(
            declared_in(name, "ZBD/zrdr.zbd", "sounds.zrd"),
            "{name} is no longer declared by sounds.zrd"
        );
    }
}

/// **The operands the follow-up named resolve to these declarations.**
///
/// Aircraft: all but `stihellhound_5_eg0` in M24's own `aiv.zrd`. Sounds: the
/// 40 `snd_` names in the global `sounds.zrd`. Animations and the leaf
/// `rock_zeppelin` in the members listed. `MSG_OBJ_DESTROY` in M24's own
/// `targets.zrd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_fu1_the_named_operands_resolve_to_these_declaring_members() {
    let own_aiv = |name: &str| declared_in(name, CONTAINER, "aiv.zrd");
    let mut aircraft: Vec<String> = (1..=6).map(|n| format!("bhatwarhawk_5_{n}")).collect();
    aircraft.extend((1..=8).map(|n| format!("stihellhound_5_{n}")));
    aircraft.extend((1..=3).map(|n| format!("bsfury_5_{n}")));
    aircraft.extend((1..=2).map(|n| format!("devastator_{n}")));
    aircraft.extend((1..=3).map(|n| format!("wingman_{n}")));
    assert_eq!(aircraft.len(), 22);
    let texts = control_texts();
    for name in &aircraft {
        assert!(texts.contains(name), "the record no longer spells {name}");
        assert!(own_aiv(name), "{name} is not declared by M24's aiv.zrd");
    }
    assert!(texts.iter().any(|text| text == "stihellhound_5_eg0"));
    assert!(!own_aiv("stihellhound_5_eg0"));

    let sounds: Vec<&String> = texts
        .iter()
        .filter(|text| text.starts_with("snd_"))
        .collect();
    assert_eq!(sounds.len(), 45, "the record's sound-name count changed");
    for name in &sounds {
        assert!(
            declared_in(name, "ZBD/zrdr.zbd", "sounds.zrd"),
            "{name} is not declared by sounds.zrd"
        );
    }

    for (name, container, member) in [
        ("call_bombs_away", CONTAINER, "glidebomb.zrd"),
        ("milesdrop", CONTAINER, "miles_drop.zrd"),
        ("killdtzep", "ZBD/zrdr.zbd", "dante_destruction.zrd"),
        ("all_dtzep_gasbags", "ZBD/zrdr.zbd", "dante_gasbags.zrd"),
        ("rock_zeppelin", "ZBD/zrdr.zbd", "rock_zeppelin.zrd"),
        ("rock_zeppelin", CONTAINER, "targets.zrd"),
        ("MSG_OBJ_DESTROY", CONTAINER, "targets.zrd"),
    ] {
        assert!(
            declared_in(name, container, member),
            "{name} is not declared by {container}#{member}"
        );
    }
}

/// **The two recorded gaps, by raw bytes.**
///
/// `stihellhound_5_eg0` occurs five times in M24's reader archive and in no
/// other file: the record is the only place it exists. `MSG_BRF_NYM4_OBJ1` is
/// spelled once in the archive and once in `strings.dll`, where the briefing
/// ids sit in a table beside `MSG_BRF_NYM3_OBJ1` and `MSG_OBJ_DESTROY`; the
/// table's format is not decoded here, so what the id resolves to stays
/// unknown. Neither is guessed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_fu1_the_undeclared_names_are_recorded_by_a_raw_byte_scan() {
    assert_eq!(
        raw_occurrences("stihellhound_5_eg0"),
        [("ZBD/C5/M04/zrdr.zbd".to_owned(), 5)],
        "stihellhound_5_eg0 appears outside M24's own archive"
    );
    assert_eq!(
        raw_occurrences("MSG_BRF_NYM4_OBJ1"),
        [
            ("ZBD/C5/M04/zrdr.zbd".to_owned(), 1),
            ("strings.dll".to_owned(), 1)
        ],
        "MSG_BRF_NYM4_OBJ1's raw spellings changed"
    );
    for sibling in ["MSG_BRF_NYM3_OBJ1", "MSG_OBJ_DESTROY"] {
        assert!(
            raw_occurrences(sibling)
                .iter()
                .any(|(file, _)| file == "strings.dll"),
            "{sibling} is not in strings.dll"
        );
    }
    // `SET_AI_` as a bare key is M24-B's recorded truncated spelling: the
    // archive spells it where the measured keys are `SET_AI_NET` and
    // `SET_AI_TEAM`, and no other member declares it.
    assert!(declarations_of("SET_AI_").is_empty());
    assert!(!declarations_of("SET_AI_NET").is_empty());
}
