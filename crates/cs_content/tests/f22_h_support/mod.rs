//! Shared measurement support for the F22-H acceptance tests and its evidence
//! harness (task #411, "Measure the original game's control vocabulary and
//! bindings").
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]` and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_f22_h_original_control_vocabulary.rs` and
//! `evidence_report_f22_h.rs` include it with `#[path = "f22_h_support/mod.rs"]`.
//!
//! **What is measured, and what is deliberately not put in this file.** The
//! original control vocabulary is measured read-only from `$CS_GAME_DIR`:
//! `strings.dll`'s `RT_STRING` labels and its `.data` `{name, id}` table, the
//! key-name pool of `crimson.icd`, and the control scripts inside
//! `GOSDATA/ASSETS/crimson.rof`. Only ids, names, byte offsets, lengths and
//! digests leave these functions; the display *text* of a label is never
//! returned or printed, because the project does not commit original content
//! (`AGENTS.md` rule 3). A label is identified by its `RT_STRING` id and its
//! measured code-unit length.
//!
//! The `.data` table is parsed here rather than through a production reader
//! because no production reader owns a PE `.data` table; every parsed id is
//! then resolved through the **production** `StringCatalog`, so a wrong parse
//! fails the acceptance test instead of merely disagreeing with itself.
//!
//! **Every control-range label is named.** The command ranges the control UI
//! reads (`10 000…10 030` and `11 000…11 100`) hold 15 and 50 labels and all
//! 65 have a `MSG_*` name in the `.data` table; the key/button range
//! (`15 000…15 141`) holds 44 and all 44 are named. So a command the original
//! exposes cannot hide behind an anonymous label: either its `MSG_*` name is
//! in this file (and in [`COMMAND_LABELS`]) or the original does not expose it.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The installation fingerprint this measurement was taken against.
pub const INSTALL_SHA256: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
pub const CONTENT_SHA256: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// `strings.dll`, whole image.
pub const STRINGS_DLL_SHA256: &str =
    "7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21";
pub const STRINGS_DLL_LEN: u64 = 131_072;

/// `crimson.icd` is the packed original game executable (`InternalName`
/// `Crimson`, `OriginalFilename` `Crimson.exe`); it carries the plaintext
/// `key_*` name pool.
pub const CRIMSON_ICD_SHA256: &str =
    "0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b";
pub const CRIMSON_ICD_LEN: u64 = 2_580_578;

/// The shared container the control UI scripts live in.
pub const CRIMSON_ROF: &str = "GOSDATA/ASSETS/crimson.rof";
pub const CRIMSON_ROF_MEMBERS: usize = 846;

/// `strings.dll`'s `RT_STRING` accounting: 112 blocks / 1 792 units, one
/// language (1033) and two non-`RT_STRING` leaves.
pub const STRINGS_STRING_UNITS: usize = 1_792;
pub const STRINGS_STRING_BLOCKS: usize = 112;
pub const STRINGS_LANGUAGE: u32 = 1033;
pub const STRINGS_OTHER_LEAVES: usize = 2;

/// The 1 023 distinct `{name, id}` entries the `.data` table holds (the table
/// is stored twice, identically, and one further printable pair, `PDT` with id
/// 0, is not an `RT_STRING` label); ids run 100 … 17 142.
pub const NAME_ID_ENTRIES: usize = 1_023;
pub const NAME_ID_MIN: u32 = 100;
pub const NAME_ID_MAX: u32 = 17_142;

/// How many `key_*` names `crimson.icd` carries.
pub const ICD_KEY_NAMES: usize = 120;
pub const ICD_KEY_POOL_START: u64 = 0x20_ce10;
pub const ICD_KEY_POOL_END: u64 = 0x20_d30a;

/// The two command-label ranges the control UI enumerates, with how many
/// non-empty labels each holds. All of them are named, so the counts and
/// [`COMMAND_LABELS`] must agree.
pub const COMMAND_RANGES: [(u32, u32, usize); 2] = [(10_000, 10_030, 15), (11_000, 11_100, 50)];
/// The key/button label range and its non-empty count.
pub const KEY_BUTTON_RANGE: (u32, u32, usize) = (15_000, 15_141, 44);

/// Every named command label the original exposes, in id order: the 15 labels
/// of the first control range and the 50 of the second. Thirty-two of these
/// correspond to a command this project also declares (see [`COMPARISON`]);
/// the other thirty-three are [`ORIGINAL_ONLY_COMMANDS`].
pub const COMMAND_LABELS: [(&str, u32); 65] = [
    ("MSG_CMD_CYCLE_MODE", 10_006),
    ("MSG_CMD_INTERP", 10_007),
    ("MSG_CMD_TARGET_NOTHING", 10_008),
    ("MSG_CMD_TARGET_UNDER_RETICULE", 10_009),
    ("MSG_CMD_NITROUS", 10_010),
    ("MSG_CMD_TARGET_NEXT_ENEMY", 10_015),
    ("MSG_CMD_TARGET_PREVIOUS_ENEMY", 10_016),
    ("MSG_CMD_TARGET_NEAREST_ENEMY", 10_017),
    ("MSG_CMD_TARGET_NEXT_ALLY", 10_018),
    ("MSG_CMD_TARGET_PREVIOUS_ALLY", 10_019),
    ("MSG_CMD_TARGET_NEAREST_ALLY", 10_020),
    ("MSG_CMD_TARGET_NEXT_GROUND", 10_021),
    ("MSG_CMD_TARGET_PREVIOUS_GROUND", 10_022),
    ("MSG_CMD_TARGET_NEAREST_GROUND", 10_023),
    ("MSG_CMD_KEYMAP_DISP", 10_025),
    ("MSG_LOOK_DOWN", 11_020),
    ("MSG_LOOK_BACK", 11_021),
    ("MSG_LOOK_LEFT", 11_022),
    ("MSG_LOOK_RIGHT", 11_023),
    ("MSG_CAM2_TOG", 11_024),
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
    ("MSG_CMD_PAUSE_GAME", 11_037),
    ("MSG_CMD_ZONE_TOGGLE", 11_040),
    ("MSG_CMD_COLLISION_TOGGLE", 11_041),
    ("MSG_CMD_TRIGGER", 11_045),
    ("MSG_CMD_BAIL_OUT", 11_053),
    ("MSG_CMD_LAUNCH_AUTO_LAND", 11_054),
    ("MSG_CMD_DISPLAY_SCORES", 11_056),
    ("MSG_NOSE_DOWN", 11_057),
    ("MSG_NOSE_UP", 11_058),
    ("MSG_ROLL_LEFT", 11_059),
    ("MSG_ROLL_RIGHT", 11_060),
    ("MSG_LEVEL_TOG", 11_061),
    ("MSG_RUDDER_LEFT", 11_062),
    ("MSG_RUDDER_RIGHT", 11_063),
    ("MSG_INC_THROTTLE", 11_067),
    ("MSG_DEC_THROTTLE", 11_068),
    ("MSG_THROTTLE_0", 11_069),
    ("MSG_THROTTLE_1", 11_070),
    ("MSG_THROTTLE_2", 11_071),
    ("MSG_THROTTLE_3", 11_072),
    ("MSG_THROTTLE_4", 11_073),
    ("MSG_THROTTLE_5", 11_074),
    ("MSG_THROTTLE_6", 11_075),
    ("MSG_THROTTLE_7", 11_076),
    ("MSG_THROTTLE_8", 11_077),
    ("MSG_MISSILE_NEXT", 11_085),
    ("MSG_FIRE_MISSILE", 11_086),
    ("MSG_CHAT_ALL", 11_087),
    ("MSG_CHAT_TEAM", 11_088),
    ("MSG_CMD_CANNON_PREV", 11_089),
    ("MSG_CMD_CANNON_NEXT", 11_090),
    ("MSG_CMD_MISSILE_PREV", 11_091),
    ("MSG_CMD_MISSILE_NEXT", 11_092),
];

/// The seven control-list category headers the rebinding UI draws (the eighth
/// hit in the name table is the dialog title, not a list header), plus the
/// four wingman order labels at 3001…3004 (`MSG_WINGMAN_*`).
pub const CONTROL_CATEGORIES: [(&str, u32); 7] = [
    ("MSG_MOVEMENT_CONTROLS", 3_005),
    ("MSG_WEAPON_CONTROLS", 3_006),
    ("MSG_VIEW1_CONTROLS", 3_007),
    ("MSG_THROTTLE_CONTROLS", 3_008),
    ("MSG_TARGETING_CONTROLS", 3_009),
    ("MSG_VIEW2_CONTROLS", 3_010),
    ("MSG_OTHER_CONTROLS", 3_011),
];

/// The two device-family option labels the control preferences screen shows.
/// The original offers a *keyboard* mode and a *keyboard+joystick* mode; there
/// is no gamepad family label and mouse/joystick buttons are bindable sources
/// rather than a third mode.
pub const DEVICE_FAMILIES: [(&str, u32); 2] =
    [("MSG_KEYBOARD", 1_023), ("MSG_KEYBOARD_JOYSTICK", 1_024)];

/// The device-source headings a binding row can name: joystick buttons, mouse
/// buttons and the two letter-key placeholders.
pub const DEVICE_SOURCES: [(&str, u32); 4] = [
    ("MSG_KEYA", 15_000),
    ("MSG_KEYB", 15_001),
    ("MSG_JOYBTN", 15_002),
    ("MSG_MOUSEBTN", 15_003),
];

/// The ten joystick-button and three mouse-button labels.
pub const BUTTON_LABELS: [(&str, u32); 13] = [
    ("MSG_JBTN_1", 15_129),
    ("MSG_JBTN_2", 15_130),
    ("MSG_JBTN_3", 15_131),
    ("MSG_JBTN_4", 15_132),
    ("MSG_JBTN_5", 15_133),
    ("MSG_JBTN_6", 15_134),
    ("MSG_JBTN_7", 15_135),
    ("MSG_JBTN_8", 15_136),
    ("MSG_JBTN_9", 15_137),
    ("MSG_JBTN_10", 15_138),
    ("MSG_MBTN_LEFT", 15_139),
    ("MSG_MBTN_RIGHT", 15_140),
    ("MSG_MBTN_MIDDLE", 15_141),
];

/// The 27 named special-key labels (the remaining 17 labels of the key/button
/// range are the four device-source headings and the thirteen button labels).
pub const KEY_LABELS: [(&str, u32); 27] = [
    ("MSG_KEY_NUMPADSUBTRACT", 15_004),
    ("MSG_KEY_NUMPADADD", 15_005),
    ("MSG_KEY_NUMPADDECIMAL", 15_006),
    ("MSG_KEY_NUMPADDIVIDE", 15_007),
    ("MSG_KEY_NUMPADMULTIPLY", 15_008),
    ("MSG_KEY_BACKSLASH", 15_048),
    ("MSG_KEY_DECIMAL", 15_081),
    ("MSG_KEY_NUMPAD1", 15_084),
    ("MSG_KEY_NUMPAD2", 15_085),
    ("MSG_KEY_NUMPAD3", 15_086),
    ("MSG_KEY_NUMPAD4", 15_087),
    ("MSG_KEY_NUMPAD5", 15_088),
    ("MSG_KEY_NUMPAD6", 15_089),
    ("MSG_KEY_NUMPAD7", 15_090),
    ("MSG_KEY_NUMPAD8", 15_091),
    ("MSG_KEY_NUMPAD9", 15_092),
    ("MSG_KEY_NUMPAD0", 15_093),
    ("MSG_KEY_NUMPADEQUALS", 15_094),
    ("MSG_KEY_NUMPADENTER", 15_095),
    ("MSG_KEY_NUMPADCOMMA", 15_096),
    ("MSG_KEY_KANA", 15_105),
    ("MSG_KEY_CONVERT", 15_106),
    ("MSG_KEY_NOCONVERT", 15_107),
    ("MSG_KEY_KANJI", 15_113),
    ("MSG_KEY_LWIN", 15_126),
    ("MSG_KEY_RWIN", 15_127),
    ("MSG_KEY_APPS", 15_128),
];

/// The control scripts inside `CRIMSON_ROF`, with their decoded length and
/// SHA-256 (reached through the production `read_tree`/`read_member`).
pub const CONTROL_SCRIPTS: [(&str, u64, &str); 3] = [
    (
        "ASSETS/SCRIPTS/CTL.SCRIPT",
        27_465,
        "8ca5a9439a2c22cb63fca166f6f6403e08b62dc30626bd32096c772d546ecb95",
    ),
    (
        "ASSETS/SCRIPTS/KEYS.SCRIPT",
        4_552,
        "abb5a703819017643587f214177cd7815b6a750e1fa89c53691cd89ebf012d29",
    ),
    (
        "ASSETS/SCRIPTS/CONTROLSPREFS.SCRIPT",
        1_385,
        "360dbe9d4f4d614310ce4db823aad577d5b7c323e36aa2be739ad2c4e327157d",
    ),
];

/// The `key_*` constants the control widget script handles itself, on top of
/// the executable's 120-name pool.
pub const CTL_SCRIPT_KEY_CONSTANTS: [&str; 10] = [
    "key_back",
    "key_delete",
    "key_end",
    "key_escape",
    "key_home",
    "key_insert",
    "key_left",
    "key_return",
    "key_right",
    "key_space",
];

/// The comparison between this project's declared vocabulary
/// (`FlightCommand::ALL` / `UiAction::ALL`) and what the original installation
/// exposes. Each entry is `(project label, status, original symbols)`.
///
/// Statuses (the task's own wording):
///
/// * `observed` — the original ships at least one named command label for this
///   control, listed in the evidence column and drawn from [`COMMAND_LABELS`];
/// * `absent` — the original exposes no command name for it. This is a strong
///   claim only because every label of the two command ranges is named, so a
///   command the original had could not hide anonymously;
/// * `runtime_only` — a menu action the original almost certainly performs but
///   which no shipped file names; it is unresolved rather than claimed.
///
/// The table is asserted to cover exactly `FlightCommand::ALL` and
/// `UiAction::ALL` by an unignored test, so the project cannot add a command
/// without classifying it against the original, and the evidence column is
/// asserted to partition [`COMMAND_LABELS`] together with
/// [`ORIGINAL_ONLY_COMMANDS`].
pub const COMPARISON: [(&str, &str, &[&str]); 25] = [
    ("pitch", "observed", &["MSG_NOSE_UP", "MSG_NOSE_DOWN"]),
    ("roll", "observed", &["MSG_ROLL_LEFT", "MSG_ROLL_RIGHT"]),
    ("yaw", "observed", &["MSG_RUDDER_LEFT", "MSG_RUDDER_RIGHT"]),
    (
        "throttle",
        "observed",
        &[
            "MSG_INC_THROTTLE",
            "MSG_DEC_THROTTLE",
            "MSG_THROTTLE_0",
            "MSG_THROTTLE_1",
            "MSG_THROTTLE_2",
            "MSG_THROTTLE_3",
            "MSG_THROTTLE_4",
            "MSG_THROTTLE_5",
            "MSG_THROTTLE_6",
            "MSG_THROTTLE_7",
            "MSG_THROTTLE_8",
        ],
    ),
    ("fire_primary", "observed", &["MSG_CMD_TRIGGER"]),
    ("fire_secondary", "observed", &["MSG_FIRE_MISSILE"]),
    (
        "cycle_weapon",
        "observed",
        &[
            "MSG_MISSILE_NEXT",
            "MSG_CMD_CANNON_PREV",
            "MSG_CMD_CANNON_NEXT",
            "MSG_CMD_MISSILE_PREV",
            "MSG_CMD_MISSILE_NEXT",
        ],
    ),
    ("drop_ordnance", "absent", &[]),
    ("eject", "observed", &["MSG_CMD_BAIL_OUT"]),
    ("toggle_gear", "absent", &[]),
    ("flap_step", "absent", &[]),
    (
        "target_next",
        "observed",
        &[
            "MSG_CMD_TARGET_NEXT_ENEMY",
            "MSG_CMD_TARGET_NEXT_ALLY",
            "MSG_CMD_TARGET_NEXT_GROUND",
        ],
    ),
    (
        "target_prev",
        "observed",
        &[
            "MSG_CMD_TARGET_PREVIOUS_ENEMY",
            "MSG_CMD_TARGET_PREVIOUS_ALLY",
            "MSG_CMD_TARGET_PREVIOUS_GROUND",
        ],
    ),
    ("countermeasure", "absent", &[]),
    ("throttle_step_up", "observed", &["MSG_INC_THROTTLE"]),
    ("throttle_step_down", "observed", &["MSG_DEC_THROTTLE"]),
    ("throttle_idle", "observed", &["MSG_THROTTLE_0"]),
    ("throttle_full", "observed", &["MSG_THROTTLE_8"]),
    ("confirm", "runtime_only", &[]),
    ("cancel", "runtime_only", &[]),
    ("navigate_up", "runtime_only", &[]),
    ("navigate_down", "runtime_only", &[]),
    ("navigate_left", "runtime_only", &[]),
    ("navigate_right", "runtime_only", &[]),
    ("pause", "observed", &["MSG_CMD_PAUSE_GAME"]),
];

/// The named original command labels with no counterpart in the project's
/// declared vocabulary. This is the other half of the measured set: 33 of the
/// 65 command labels are original-only.
pub const ORIGINAL_ONLY_COMMANDS: [&str; 33] = [
    "MSG_CMD_CYCLE_MODE",
    "MSG_CMD_INTERP",
    "MSG_CMD_TARGET_NOTHING",
    "MSG_CMD_TARGET_UNDER_RETICULE",
    "MSG_CMD_NITROUS",
    "MSG_CMD_TARGET_NEAREST_ENEMY",
    "MSG_CMD_TARGET_NEAREST_ALLY",
    "MSG_CMD_TARGET_NEAREST_GROUND",
    "MSG_CMD_KEYMAP_DISP",
    "MSG_LOOK_DOWN",
    "MSG_LOOK_BACK",
    "MSG_LOOK_LEFT",
    "MSG_LOOK_RIGHT",
    "MSG_CAM2_TOG",
    "MSG_CMD_PADLOCK_SNAP",
    "MSG_CMD_PADLOCK_WATCH",
    "MSG_PADLOCK_DL",
    "MSG_PADLOCK_D",
    "MSG_PADLOCK_DR",
    "MSG_PADLOCK_L",
    "MSG_PADLOCK_M",
    "MSG_PADLOCK_R",
    "MSG_PADLOCK_UL",
    "MSG_PADLOCK_U",
    "MSG_PADLOCK_UR",
    "MSG_CMD_PADLOCK_STICK",
    "MSG_CMD_ZONE_TOGGLE",
    "MSG_CMD_COLLISION_TOGGLE",
    "MSG_CMD_LAUNCH_AUTO_LAND",
    "MSG_CMD_DISPLAY_SCORES",
    "MSG_LEVEL_TOG",
    "MSG_CHAT_ALL",
    "MSG_CHAT_TEAM",
];

/// The read-only installation root, or a loud failure: a retail test must
/// fail, not pass, when `CS_GAME_DIR` is absent.
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

/// The bytes of the installation file `spelling` names (a `/`-separated path
/// relative to the installation root).
pub fn read_installation_file(dir: &Path, spelling: &str) -> Vec<u8> {
    let mut path = dir.to_path_buf();
    for segment in spelling.split('/') {
        path.push(segment);
    }
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{spelling}: the installation must hold it: {error}"))
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
            if let Some(name) = zero_terminated_ascii(strings, at) {
                if (2..=60).contains(&name.len()) {
                    table.insert(name, id);
                }
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

/// Every distinct `key_*` name in `crimson.icd`'s pool, in file order.
pub fn parse_icd_key_names(icd: &[u8]) -> Vec<(u64, String)> {
    let mut names: Vec<(u64, String)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut cursor = 0usize;
    while let Some(found) = icd[cursor..].windows(4).position(|w| w == b"key_") {
        let at = cursor + found;
        let Some(name) = zero_terminated_ascii(icd, at) else {
            cursor = at + 4;
            continue;
        };
        if seen.insert(name.clone()) {
            names.push((at as u64, name));
        }
        cursor = at + 4;
    }
    names
}

/// The id [`COMMAND_LABELS`] gives `name`, or `None` when the original does
/// not name a command like that.
pub fn command_label_id(name: &str) -> Option<u32> {
    COMMAND_LABELS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, id)| *id)
}

/// The comparison entry for a project label: its status and the original
/// symbols that justify it.
pub fn comparison_entry(label: &str) -> Option<(&'static str, &'static [&'static str])> {
    COMPARISON
        .iter()
        .find(|(candidate, _, _)| *candidate == label)
        .map(|(_, status, evidence)| (*status, *evidence))
}
