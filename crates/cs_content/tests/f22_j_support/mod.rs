//! Shared measurement support for the F22-J acceptance tests and its evidence
//! harness (task #505, "Recover the original default control bindings").
//!
//! It is a `tests/` subdirectory module, not a test target of its own: it has
//! no `#[test]` and cargo never discovers `tests/<dir>/mod.rs` as a binary.
//! Both `accept_f22_j_original_default_bindings.rs` and
//! `evidence_report_f22_j.rs` include it with
//! `#[path = "f22_j_support/mod.rs"]`.
//!
//! **What is measured here, and with what method.** The original's default
//! control table is code in the owner-supplied decrypted engine image
//! (`$CS_ENGINE_IMAGE`, the decrypted `crimson.icd`), built by the function at
//! `0x4936c0`–`0x493d00`. This module carries:
//!
//! 1. [`MEASURED_ROWS`], the recorded measurement — 64 rows of command id,
//!    label id, key codes, joystick button, mouse button and the virtual
//!    address of the registration `call`. It is transcribed from the owner's
//!    static analysis (Rally #505 owner note, 2026-10-05) and confirmed by
//!    this task's own read-only scan of the image ([`decode_registrations`]),
//!    which is the same scan the retail acceptance test runs;
//! 2. [`decode_registrations`], the bounded x86 scanner itself: it finds every
//!    `call 0x493630` inside the table span and decodes the six `push`
//!    operands that precede it;
//! 3. the checks behind the two analog defaults and the Japanese-keyboard
//!    remap.
//!
//! **Commit rules.** Only ids, codes, addresses, spans and digests are
//! recorded here. No image bytes are stored as constants, no executable is
//! committed and no original display text is reproduced: the expected
//! Japanese-remap instruction bytes are *derived* from the recorded code
//! pairs, never copied out of the image.
//!
//! F22-H's measurement support is reused, not re-stated: the 65 measured
//! command labels, the category headers and the installation fingerprints
//! come from `f22_h_support`, which the test binaries include alongside this
//! module, so the "which of F22-H's labels are on the controls screen"
//! recomputation compares against F22-H's own list.

#![allow(dead_code)]

use cs_types::input::{
    ORIGINAL_IMAGE_VA_BIAS, ORIGINAL_REGISTER_VA, ORIGINAL_TABLE_SPAN, OriginalControlCategory,
};

/// The four command labels the controls screen enumerates whose ids lie
/// outside the two ranges F22-H measured. Their symbols and ids are read from
/// `strings.dll`'s `.data` `{name, id}` table by the retail test.
pub const SCREEN_ONLY_LABELS: [(&str, u32); 4] = [
    ("MSG_LOOK_FORWARD", 158),
    ("MSG_LOOK_FLYBY", 143),
    ("MSG_CAM_ZOOM_IN", 175),
    ("MSG_CAM_ZOOM_OUT", 176),
];

/// The five command labels F22-H measured that the controls screen does
/// **not** enumerate, recomputed against F22-H's 65-label list: exactly these
/// five leave the measured set when the screen's rows are removed.
pub const NOT_ON_SCREEN_LABELS: [(&str, u32); 5] = [
    ("MSG_CMD_CYCLE_MODE", 10_006),
    ("MSG_CMD_INTERP", 10_007),
    ("MSG_CMD_ZONE_TOGGLE", 11_040),
    ("MSG_CMD_COLLISION_TOGGLE", 11_041),
    ("MSG_MISSILE_NEXT", 11_085),
];

/// One recorded measurement row: `(command_id, label_id, category, key_a,
/// key_b, joystick_button, mouse_button, registration_va)`.
///
/// Key codes are the original's packed values (a DirectInput scancode plus
/// the modifier bits `0x100` Alt, `0x200` Ctrl, `0x400` Shift); `0` is an
/// unbound slot. The joystick button is 1-based, the mouse button is `1`
/// left / `2` right / `3` middle.
pub type MeasuredRow = (u16, u32, OriginalControlCategory, u16, u16, u8, u8, u32);

/// The recorded measurement: one entry per registration call in the table
/// span, in the order the screen enumerates them.
///
/// The values come from the owner's static analysis of the decrypted image
/// and were confirmed by [`decode_registrations`] over the same image; this
/// constant is the recorded expectation the production
/// [`ORIGINAL_DEFAULT_BINDINGS`](cs_types::input::ORIGINAL_DEFAULT_BINDINGS)
/// is pinned against, so a transcription error in either fails the test.
pub const MEASURED_ROWS: [MeasuredRow; 64] = [
    (
        1,
        11057,
        OriginalControlCategory::Movement,
        0x0c8,
        0x000,
        0,
        0,
        0x00493718,
    ),
    (
        2,
        11058,
        OriginalControlCategory::Movement,
        0x0d0,
        0x000,
        0,
        0,
        0x0049372f,
    ),
    (
        3,
        11059,
        OriginalControlCategory::Movement,
        0x0cb,
        0x000,
        0,
        0,
        0x00493746,
    ),
    (
        4,
        11060,
        OriginalControlCategory::Movement,
        0x0cd,
        0x000,
        0,
        0,
        0x00493760,
    ),
    (
        5,
        11062,
        OriginalControlCategory::Movement,
        0x033,
        0x052,
        0,
        0,
        0x00493774,
    ),
    (
        6,
        11063,
        OriginalControlCategory::Movement,
        0x034,
        0x053,
        0,
        0,
        0x00493788,
    ),
    (
        47,
        11061,
        OriginalControlCategory::Movement,
        0x426,
        0x000,
        0,
        0,
        0x004937a2,
    ),
    (
        7,
        11067,
        OriginalControlCategory::Throttle,
        0x00d,
        0x000,
        0,
        0,
        0x004937cb,
    ),
    (
        8,
        11068,
        OriginalControlCategory::Throttle,
        0x00c,
        0x000,
        0,
        0,
        0x004937df,
    ),
    (
        9,
        11069,
        OriginalControlCategory::Throttle,
        0x002,
        0x000,
        0,
        0,
        0x004937f6,
    ),
    (
        10,
        11070,
        OriginalControlCategory::Throttle,
        0x003,
        0x000,
        0,
        0,
        0x0049380a,
    ),
    (
        11,
        11071,
        OriginalControlCategory::Throttle,
        0x004,
        0x000,
        0,
        0,
        0x0049381e,
    ),
    (
        12,
        11072,
        OriginalControlCategory::Throttle,
        0x005,
        0x000,
        0,
        0,
        0x00493835,
    ),
    (
        13,
        11073,
        OriginalControlCategory::Throttle,
        0x006,
        0x000,
        0,
        0,
        0x00493849,
    ),
    (
        14,
        11074,
        OriginalControlCategory::Throttle,
        0x007,
        0x000,
        0,
        0,
        0x0049385d,
    ),
    (
        15,
        11075,
        OriginalControlCategory::Throttle,
        0x008,
        0x000,
        0,
        0,
        0x00493874,
    ),
    (
        16,
        11076,
        OriginalControlCategory::Throttle,
        0x009,
        0x000,
        0,
        0,
        0x00493888,
    ),
    (
        17,
        11077,
        OriginalControlCategory::Throttle,
        0x00a,
        0x000,
        0,
        0,
        0x0049389c,
    ),
    (
        19,
        11045,
        OriginalControlCategory::Weapons,
        0x039,
        0x000,
        1,
        0,
        0x004938c8,
    ),
    (
        20,
        11086,
        OriginalControlCategory::Weapons,
        0x02d,
        0x000,
        2,
        0,
        0x004938dc,
    ),
    (
        25,
        11089,
        OriginalControlCategory::Weapons,
        0x03d,
        0x000,
        0,
        0,
        0x004938f0,
    ),
    (
        26,
        11090,
        OriginalControlCategory::Weapons,
        0x03e,
        0x000,
        7,
        0,
        0x00493907,
    ),
    (
        32,
        11091,
        OriginalControlCategory::Weapons,
        0x03f,
        0x000,
        0,
        0,
        0x0049391b,
    ),
    (
        33,
        11092,
        OriginalControlCategory::Weapons,
        0x040,
        0x000,
        8,
        0,
        0x0049392f,
    ),
    (
        36,
        10015,
        OriginalControlCategory::Targeting,
        0x012,
        0x000,
        3,
        0,
        0x0049395b,
    ),
    (
        37,
        10016,
        OriginalControlCategory::Targeting,
        0x412,
        0x000,
        0,
        0,
        0x00493972,
    ),
    (
        38,
        10017,
        OriginalControlCategory::Targeting,
        0x212,
        0x000,
        0,
        0,
        0x00493989,
    ),
    (
        39,
        10018,
        OriginalControlCategory::Targeting,
        0x011,
        0x000,
        0,
        0,
        0x004939a0,
    ),
    (
        40,
        10019,
        OriginalControlCategory::Targeting,
        0x411,
        0x000,
        0,
        0,
        0x004939b7,
    ),
    (
        41,
        10020,
        OriginalControlCategory::Targeting,
        0x211,
        0x000,
        0,
        0,
        0x004939ce,
    ),
    (
        42,
        10021,
        OriginalControlCategory::Targeting,
        0x013,
        0x000,
        6,
        0,
        0x004939e5,
    ),
    (
        43,
        10022,
        OriginalControlCategory::Targeting,
        0x413,
        0x000,
        0,
        0,
        0x004939fc,
    ),
    (
        44,
        10023,
        OriginalControlCategory::Targeting,
        0x213,
        0x000,
        0,
        0,
        0x00493a13,
    ),
    (
        45,
        10009,
        OriginalControlCategory::Targeting,
        0x010,
        0x000,
        0,
        0,
        0x00493a2a,
    ),
    (
        46,
        10008,
        OriginalControlCategory::Targeting,
        0x014,
        0x000,
        0,
        0,
        0x00493a3e,
    ),
    (
        48,
        11024,
        OriginalControlCategory::View1,
        0x41f,
        0x000,
        5,
        0,
        0x00493a6a,
    ),
    (
        49,
        158,
        OriginalControlCategory::View1,
        0x042,
        0x000,
        4,
        0,
        0x00493a81,
    ),
    (
        50,
        11020,
        OriginalControlCategory::View1,
        0x043,
        0x000,
        0,
        0,
        0x00493a95,
    ),
    (
        51,
        11021,
        OriginalControlCategory::View1,
        0x044,
        0x000,
        0,
        0,
        0x00493aa9,
    ),
    (
        52,
        11022,
        OriginalControlCategory::View1,
        0x057,
        0x000,
        0,
        0,
        0x00493ac0,
    ),
    (
        53,
        11023,
        OriginalControlCategory::View1,
        0x058,
        0x000,
        0,
        0,
        0x00493ad4,
    ),
    (
        54,
        143,
        OriginalControlCategory::View1,
        0x041,
        0x000,
        0,
        0,
        0x00493ae8,
    ),
    (
        55,
        11025,
        OriginalControlCategory::View1,
        0x025,
        0x000,
        0,
        0,
        0x00493aff,
    ),
    (
        57,
        11036,
        OriginalControlCategory::View1,
        0x024,
        0x000,
        0,
        0,
        0x00493b13,
    ),
    (
        56,
        11026,
        OriginalControlCategory::View1,
        0x026,
        0x000,
        0,
        0,
        0x00493b27,
    ),
    (
        58,
        11027,
        OriginalControlCategory::View2,
        0x04f,
        0x000,
        0,
        0,
        0x00493b53,
    ),
    (
        59,
        11028,
        OriginalControlCategory::View2,
        0x050,
        0x000,
        0,
        0,
        0x00493b67,
    ),
    (
        60,
        11029,
        OriginalControlCategory::View2,
        0x051,
        0x000,
        0,
        0,
        0x00493b7b,
    ),
    (
        61,
        11030,
        OriginalControlCategory::View2,
        0x04b,
        0x000,
        0,
        0,
        0x00493b92,
    ),
    (
        62,
        11031,
        OriginalControlCategory::View2,
        0x04c,
        0x000,
        0,
        0,
        0x00493ba6,
    ),
    (
        63,
        11032,
        OriginalControlCategory::View2,
        0x04d,
        0x000,
        0,
        0,
        0x00493bba,
    ),
    (
        64,
        11033,
        OriginalControlCategory::View2,
        0x047,
        0x000,
        0,
        0,
        0x00493bd1,
    ),
    (
        65,
        11034,
        OriginalControlCategory::View2,
        0x048,
        0x000,
        0,
        0,
        0x00493be5,
    ),
    (
        66,
        11035,
        OriginalControlCategory::View2,
        0x049,
        0x000,
        0,
        0,
        0x00493bf9,
    ),
    (
        68,
        176,
        OriginalControlCategory::View2,
        0x04a,
        0x000,
        0,
        0,
        0x00493c10,
    ),
    (
        67,
        175,
        OriginalControlCategory::View2,
        0x04e,
        0x000,
        0,
        0,
        0x00493c24,
    ),
    (
        106,
        11054,
        OriginalControlCategory::Other,
        0x01e,
        0x000,
        0,
        0,
        0x00493c4d,
    ),
    (
        18,
        10010,
        OriginalControlCategory::Other,
        0x031,
        0x000,
        0,
        0,
        0x00493c64,
    ),
    (
        34,
        11053,
        OriginalControlCategory::Other,
        0x22d,
        0x000,
        0,
        0,
        0x00493c7b,
    ),
    (
        69,
        11037,
        OriginalControlCategory::Other,
        0x001,
        0x000,
        0,
        0,
        0x00493c8f,
    ),
    (
        35,
        11056,
        OriginalControlCategory::Other,
        0x00f,
        0x000,
        0,
        0,
        0x00493ca6,
    ),
    (
        109,
        11087,
        OriginalControlCategory::Other,
        0x029,
        0x000,
        0,
        0,
        0x00493cba,
    ),
    (
        110,
        11088,
        OriginalControlCategory::Other,
        0x429,
        0x000,
        0,
        0,
        0x00493cd1,
    ),
    (
        111,
        10025,
        OriginalControlCategory::Other,
        0x03b,
        0x000,
        0,
        0,
        0x00493ce8,
    ),
];

/// One registration call decoded from the image: the six operands the
/// registration function is called with, plus the `call`'s own virtual
/// address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registration {
    /// The command id (the first argument).
    pub command_id: u16,
    /// The label string id (the second argument).
    pub label_id: u32,
    /// The primary key code (the third argument).
    pub key_a: u16,
    /// The secondary key code (the fourth argument).
    pub key_b: u16,
    /// The joystick button (the fifth argument).
    pub joystick_button: u8,
    /// The mouse button (the sixth argument).
    pub mouse_button: u8,
    /// The virtual address of the `call` opcode.
    pub call_va: u32,
}

/// Decodes every registration call inside [`ORIGINAL_TABLE_SPAN`] of `image`.
///
/// The scan is bounded and declarative: it looks for a `call rel32` whose
/// target is the registration function, then walks backwards over the six
/// `push` operands the C calling convention passes in reverse order
/// (`mouse_button`, `joystick_button`, `key_b`, `key_a`, `label_id`,
/// `command_id`), skipping the `mov [imm32], eax` stores some rows carry.
///
/// A row it cannot decode this way — a register push, an unexpected
/// instruction — is **left out**, so a scan that meets anything other than
/// the plain immediate pushes returns fewer rows and the caller's count
/// assertion fails instead of silently passing.
pub fn decode_registrations(image: &[u8]) -> Vec<Registration> {
    let (span_start, span_end) = ORIGINAL_TABLE_SPAN;
    let start = (span_start - ORIGINAL_IMAGE_VA_BIAS) as usize;
    let end = (span_end - ORIGINAL_IMAGE_VA_BIAS) as usize;
    let span = image.get(start..end).unwrap_or_else(|| {
        panic!("the image must cover the table span {span_start:#x}..{span_end:#x}")
    });

    let mut rows = Vec::new();
    let mut cursor = 0usize;
    while cursor + 5 <= span.len() {
        if span[cursor] != 0xe8 {
            cursor += 1;
            continue;
        }
        let displacement = i32::from_le_bytes([
            span[cursor + 1],
            span[cursor + 2],
            span[cursor + 3],
            span[cursor + 4],
        ]);
        let next_va = i64::from(span_start) + cursor as i64 + 5;
        if next_va + i64::from(displacement) == i64::from(ORIGINAL_REGISTER_VA)
            && let Some(row) = decode_row(span, cursor, span_start)
        {
            rows.push(row);
        }
        cursor += 1;
    }
    rows
}

/// Decodes the six pushes ending at `call` (the index of a `call` opcode) and
/// returns the registration they make, or `None` when the walk meets an
/// instruction this scanner does not model.
fn decode_row(span: &[u8], call: usize, span_start: u32) -> Option<Registration> {
    // The pushes in reverse order: mouse, joy, key_b, key_a, label, command.
    let mut operands = [0u32; 6];
    let mut found = 0usize;
    let mut cursor = call;
    while found < 6 {
        let five_before = cursor.checked_sub(5).map(|at| span[at]);
        if five_before == Some(0x68) {
            // push imm32
            operands[found] = u32::from_le_bytes(span[cursor - 4..cursor].try_into().ok()?);
            cursor -= 5;
        } else if five_before == Some(0xa3) {
            // mov [imm32], eax: a store between the pushes, not an operand.
            cursor -= 5;
            continue;
        } else if cursor >= 2 && span[cursor - 2] == 0x6a {
            // push imm8
            operands[found] = u32::from(span[cursor - 1]);
            cursor -= 2;
        } else if cursor >= 1 && (0x50..=0x57).contains(&span[cursor - 1]) {
            // A register push: the value is not an immediate, so this row is
            // not decodable from the image alone.
            return None;
        } else {
            return None;
        }
        found += 1;
    }
    // Walking backwards meets the arguments in reverse push order, so the
    // first operand decoded is the last argument: command, label, key_a,
    // key_b, joystick button, mouse button.
    let [command, label, key_a, key_b, joystick, mouse] = operands;
    Some(Registration {
        command_id: u16::try_from(command).ok()?,
        label_id: label,
        key_a: u16::try_from(key_a).ok()?,
        key_b: u16::try_from(key_b).ok()?,
        joystick_button: u8::try_from(joystick).ok()?,
        mouse_button: u8::try_from(mouse).ok()?,
        call_va: span_start + call as u32,
    })
}

/// The instruction bytes the Japanese-keyboard remap function at
/// `ORIGINAL_JAPANESE_REMAP_VA` must hold, derived from the recorded
/// `(input, remapped)` code pairs.
///
/// The bytes are *constructed* from the recorded codes — `cmp`/`je`/`jne`
/// opcodes with the recorded immediates — never copied out of the image, so
/// the check commits ids and codes only. A different remap table produces
/// different bytes and fails the check.
pub fn expected_japanese_remap_bytes(pairs: [(u16, u16); 3]) -> Vec<u8> {
    let [
        (input_a, remapped_a),
        (input_b, remapped_b),
        (input_c, remapped_c),
    ] = pairs;
    let mut bytes = vec![
        0x8b,
        0x44,
        0x24,
        0x04, // mov eax, [esp+4]
        0x83,
        0xf8, // cmp eax, imm8 (input a)
        input_a as u8,
        0x74,
        0x18, // je -> the remapped-a block
        0x83,
        0xf8, // cmp eax, imm8 (input b)
        input_b as u8,
        0x74,
        0x0d, // je -> the remapped-b block
        0x3d, // cmp eax, imm32 (input c)
    ];
    bytes.extend_from_slice(&u32::from(input_c).to_le_bytes());
    bytes.extend_from_slice(&[
        0x75, 0x11, // jne -> the identity return
        0xb8, // mov eax, imm32 (remapped c)
    ]);
    bytes.extend_from_slice(&u32::from(remapped_c).to_le_bytes());
    bytes.extend_from_slice(&[0xc3]); // ret
    bytes.extend_from_slice(&[0xb8]); // mov eax, imm32 (remapped b)
    bytes.extend_from_slice(&u32::from(remapped_b).to_le_bytes());
    bytes.extend_from_slice(&[0xc3]); // ret
    bytes.extend_from_slice(&[0xb8]); // mov eax, imm32 (remapped a)
    bytes.extend_from_slice(&u32::from(remapped_a).to_le_bytes());
    bytes.extend_from_slice(&[0xc3]); // ret
    bytes
}

/// The bits of the f32 `value`, as the image stores them.
pub fn f32_bits(value: f32) -> [u8; 4] {
    value.to_le_bytes()
}
