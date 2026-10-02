//! Shared measurement support for the F30-D acceptance tests and its evidence
//! harness (task #124, "Verify target order, reveal rules and original
//! assistance behavior", `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`
//! stage `### F30-D`).
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]`, and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_f30_d_original_target_vocabulary.rs` and
//! `evidence_report_f30_d.rs` include it with `#[path = "f30_d_support/mod.rs"]`.
//!
//! # What is measured, and what deliberately never leaves this file
//!
//! The original 2000 PC Crimson Skies keeps the *names* of its target
//! commands, its targeting control category, its target-display text fonts and
//! its padlock command family in `strings.dll`: a `{name, id}` table in the
//! image's PE `.data` section plus the `RT_STRING` label tree. That table is
//! read here, read-only, from `$CS_GAME_DIR`.
//!
//! Only ids, symbolic names, measured counts and digests leave these
//! functions. The **display text** of a label is never returned, printed or
//! committed (`AGENTS.md` rule 3: no original content in Git), exactly as
//! F22-H did for the control vocabulary. A label is identified by its
//! `RT_STRING` id, its symbolic name and its measured code-unit length.
//!
//! # Why the table is parsed here
//!
//! No production reader owns a PE `.data` section, so the `{pointer, id}`
//! table is parsed test-locally. Every id it returns is then resolved through
//! the **production** `cs_content::config::StringCatalog`, so a wrong parse
//! fails the acceptance test instead of merely agreeing with itself. The
//! installation itself is fingerprinted with the production
//! `cs_assets::install` readers.
//!
//! # What is *not* observable here, and therefore not asserted
//!
//! The original's **target order**, its **reveal rules** and its
//! **assistance behavior** are not in any shipped readable file: the control
//! UI fetches every command row and its current binding from native callbacks
//! in the packed executable (F22-H, Observation 3), and the labels measured
//! here name *which* commands exist, never what they do. Every statement in
//! this file is therefore about the measured **vocabulary**, and the
//! classification table states per action whether the original names it. An
//! absent name is "absent from the shipped observation", never "the original
//! cannot do it".

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The installation fingerprint this measurement was taken against.
pub const INSTALL_SHA256: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
pub const CONTENT_SHA256: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// `strings.dll`, whole image: the file that carries both the `RT_STRING`
/// label tree and the `{name, id}` table this module reads.
pub const STRINGS_DLL_SHA256: &str =
    "7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21";
pub const STRINGS_DLL_LEN: u64 = 131_072;

/// `strings.dll`'s `RT_STRING` accounting on this installation: 112 blocks,
/// 1 792 string units, one language (1033) and two non-`RT_STRING` leaves.
pub const STRINGS_STRING_UNITS: usize = 1_792;
pub const STRINGS_STRING_BLOCKS: usize = 112;
pub const STRINGS_LANGUAGE: u32 = 1033;
pub const STRINGS_OTHER_LEAVES: usize = 2;

/// The 1 023 distinct `{name, id}` entries the `.data` table holds (the table
/// is stored twice, identically; one further printable pair, `PDT` with id 0,
/// is not an `RT_STRING` label). Ids run 100 … 17 142.
pub const NAME_ID_ENTRIES: usize = 1_023;
pub const NAME_ID_MIN: u32 = 100;
pub const NAME_ID_MAX: u32 = 17_142;

/// The targeting control-category header the original's rebinding UI draws
/// under the target commands.
pub const TARGETING_CATEGORY: (&str, u32) = ("MSG_TARGETING_CONTROLS", 3_009);

/// The **target command labels** the original names, in id order.
///
/// Eleven of them: a *clear* (`TARGET_NOTHING`, 10 008), an *under-crosshair*
/// pick (`TARGET_UNDER_RETICULE`, 10 009) and, for each of three named target
/// classes — enemy, ally, ground — a next / previous / nearest triple. The
/// project models that shape as [`TargetFilter`] axes over one cycle action;
/// the labels name the classes, never the order the cycle walks them in.
pub const TARGET_COMMAND_LABELS: [(&str, u32); 11] = [
    ("MSG_CMD_TARGET_NOTHING", 10_008),
    ("MSG_CMD_TARGET_UNDER_RETICULE", 10_009),
    ("MSG_CMD_TARGET_NEXT_ENEMY", 10_015),
    ("MSG_CMD_TARGET_PREVIOUS_ENEMY", 10_016),
    ("MSG_CMD_TARGET_NEAREST_ENEMY", 10_017),
    ("MSG_CMD_TARGET_NEXT_ALLY", 10_018),
    ("MSG_CMD_TARGET_PREVIOUS_ALLY", 10_019),
    ("MSG_CMD_TARGET_NEAREST_ALLY", 10_020),
    ("MSG_CMD_TARGET_NEXT_GROUND", 10_021),
    ("MSG_CMD_TARGET_PREVIOUS_GROUND", 10_022),
    ("MSG_CMD_TARGET_NEAREST_GROUND", 10_023),
];

/// The **padlock** command labels: three mode commands and nine directions.
///
/// This is the assist-shaped vocabulary the original *does* name. What a mode
/// or a direction does is native, so these names are evidence that an assist
/// family exists, never evidence of an aim correction (F30 non-negotiable 3).
pub const PADLOCK_COMMAND_LABELS: [(&str, u32); 12] = [
    ("MSG_CMD_PADLOCK_SNAP", 11_025),
    ("MSG_CMD_PADLOCK_WATCH", 11_026),
    ("MSG_PADLOCK_DL", 11_027),
    ("MSG_PADLOCK_D", 11_028),
    ("MSG_PADLOCK_DR", 11_029),
    ("MSG_PADLOCK_L", 11_030),
    ("MSG_PADLOCK_M", 11_031),
    ("MSG_PADLOCK_R", 11_032),
    ("MSG_PADLOCK_UL", 11_033),
    ("MSG_PADLOCK_U", 11_034),
    ("MSG_PADLOCK_UR", 11_035),
    ("MSG_CMD_PADLOCK_STICK", 11_036),
];

/// The target-display text fonts the original's string image names, with the
/// aim-point font. A font name is evidence that the original's HUD has that
/// *text element*; it is not evidence of what is drawn in it, and not evidence
/// that an aid is computed.
pub const TARGET_FONTS: [(&str, u32); 4] = [
    ("FONT_AIMPOINT", 17_004),
    ("FONT_TARGETHELP", 17_010),
    ("FONT_TARGETTITLE", 17_011),
    ("FONT_TARGETPOS", 17_012),
];

/// The name fragments whose complete **absence** is measured: no name in the
/// 1 023-entry table contains any of them, so the shipped string image names no
/// reveal/visibility concept and no lead-indicator or aim-assistance option.
///
/// This is an absence measurement over a table in which every entry is named,
/// which is what makes it a strong claim — but only about the *string
/// vocabulary*. The behavior those concepts would implement is native.
pub const ABSENT_NAME_FRAGMENTS: [&str; 8] = [
    "REVEAL", "VISIB", "SENSOR", "DETECT", "HIDDEN", "STEALTH", "LEAD", "ASSIST",
];

/// The classification of this project's declared actions against the measured
/// original vocabulary. One entry per
/// [`cs_content::target_rules::DeclaredAction`], so the project cannot add a
/// targeting action without classifying it.
///
/// Statuses:
///
/// * `observed` — the original ships a named command label for the action,
///   listed in the evidence column and drawn from
///   [`TARGET_COMMAND_LABELS`];
/// * `absent` — the original names no command for it. Strong only because
///   every target-family label above is named, so an action the original had
///   could not hide anonymously; it still means "absent from the shipped
///   observation".
pub const ACTION_CLASSIFICATION: [(&str, &str, &[&str]); 9] = [
    (
        "next_hostile",
        "observed",
        &[
            "MSG_CMD_TARGET_NEXT_ENEMY",
            "MSG_CMD_TARGET_NEXT_ALLY",
            "MSG_CMD_TARGET_NEXT_GROUND",
        ],
    ),
    (
        "previous_hostile",
        "observed",
        &[
            "MSG_CMD_TARGET_PREVIOUS_ENEMY",
            "MSG_CMD_TARGET_PREVIOUS_ALLY",
            "MSG_CMD_TARGET_PREVIOUS_GROUND",
        ],
    ),
    (
        "nearest_hostile",
        "observed",
        &[
            "MSG_CMD_TARGET_NEAREST_ENEMY",
            "MSG_CMD_TARGET_NEAREST_ALLY",
            "MSG_CMD_TARGET_NEAREST_GROUND",
        ],
    ),
    ("nearest_objective", "absent", &[]),
    ("nearest_ally", "observed", &["MSG_CMD_TARGET_NEAREST_ALLY"]),
    (
        "nearest_non_aircraft",
        "observed",
        &["MSG_CMD_TARGET_NEAREST_GROUND"],
    ),
    ("nearest_attacker", "absent", &[]),
    (
        "under_crosshair",
        "observed",
        &["MSG_CMD_TARGET_UNDER_RETICULE"],
    ),
    ("clear", "observed", &["MSG_CMD_TARGET_NOTHING"]),
];

/// The read-only installation root, or a loud failure: a retail test must fail,
/// not pass, when `CS_GAME_DIR` is absent.
pub fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this test needs the original installation \
         (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The `{name, id}` table `strings.dll` keeps in its PE `.data` section.
///
/// Each entry is two little-endian `u32`s: a pointer into the image's own
/// string pool (an `ImageBase`-relative virtual address) and the `RT_STRING`
/// id that name labels. The table is stored twice, identically; the returned
/// map is the deduplicated union. A row whose id is below [`NAME_ID_MIN`] is
/// not an `RT_STRING` label (`strings.dll` also holds one printable `PDT`
/// pair with id 0) and is dropped.
pub fn parse_name_id_table(strings: &[u8]) -> BTreeMap<String, u32> {
    let e_lfanew = u32::from_le_bytes(strings[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(
        &strings[e_lfanew..e_lfanew + 4],
        b"PE\0\0",
        "strings.dll must be a PE image"
    );
    let number_of_sections =
        u16::from_le_bytes(strings[e_lfanew + 6..e_lfanew + 8].try_into().unwrap()) as usize;
    let size_of_optional_header =
        u16::from_le_bytes(strings[e_lfanew + 20..e_lfanew + 22].try_into().unwrap()) as usize;
    let optional = e_lfanew + 24;
    let magic = u16::from_le_bytes(strings[optional..optional + 2].try_into().unwrap());
    assert_eq!(magic, 0x10b, "strings.dll must be PE32");
    let image_base = u32::from_le_bytes(strings[optional + 28..optional + 32].try_into().unwrap());

    let section_table = optional + size_of_optional_header;
    let mut data_extent = None;
    for index in 0..number_of_sections {
        let section = section_table + index * 40;
        let name = strings[section..section + 8]
            .split(|byte| *byte == 0)
            .next();
        if name == Some(&b".data"[..]) {
            let virtual_address =
                u32::from_le_bytes(strings[section + 12..section + 16].try_into().unwrap());
            let raw_size =
                u32::from_le_bytes(strings[section + 16..section + 20].try_into().unwrap());
            let raw_offset =
                u32::from_le_bytes(strings[section + 20..section + 24].try_into().unwrap());
            data_extent = Some((virtual_address, raw_offset, raw_size));
        }
    }
    let (data_va, data_offset, data_size) =
        data_extent.expect("strings.dll must carry a .data section");
    let low = image_base + data_va;
    let high = low + data_size;

    let mut table = BTreeMap::new();
    let mut offset = data_offset as usize;
    let end = (data_offset + data_size) as usize - 8;
    while offset <= end {
        let pointer = u32::from_le_bytes(strings[offset..offset + 4].try_into().unwrap());
        let id = u32::from_le_bytes(strings[offset + 4..offset + 8].try_into().unwrap());
        if pointer >= low && pointer < high && (NAME_ID_MIN..100_000).contains(&id) {
            let at = (pointer - image_base) as usize;
            if let Some(name) = zero_terminated_ascii(strings, at)
                && (2..=60).contains(&name.len())
            {
                table.insert(name, id);
            }
        }
        offset += 8;
    }
    table
}

/// The ASCII, NUL-terminated string at `offset`, or `None` when it does not
/// look like a printable name.
fn zero_terminated_ascii(bytes: &[u8], offset: usize) -> Option<String> {
    let end = bytes
        .get(offset..)?
        .iter()
        .position(|byte| *byte == 0)
        .map(|length| offset + length)?;
    let name = std::str::from_utf8(&bytes[offset..end]).ok()?;
    if name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        Some(name.to_owned())
    } else {
        None
    }
}
