//! M01-WINGMAN-STICK-CALLSITE (#2276): which `0x41b560` call site a mode-4
//! station-flying follower reaches, what that site passes as the law's
//! fourth argument (`bl`, gated at `0x41be47`), and whether the vehicle
//! definition's `kind_of` key merges the parent's parsed fields.
//!
//! Task test prefix: `accept_m01_wingman_callsite_`. The answers, with the
//! byte windows these tests pin, are in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §6:
//!
//! * the AI step `0x41c270` dispatches `[ai+0x67c] == 4` to the follower
//!   routine `0x41e760` (`0x41c2dd`/`0x41c358`/`0x41c359`);
//! * inside it the station call is `0x41eed3`, reached for every follower
//!   sub-state `[ai+0x358]` outside 3 (escape) and 4 (stunned), and it
//!   pushes `bl = 0` — the normal `vehicle+0x8c4..+0x8d8` gains, never the
//!   `ai_emerg_input_*` set; the escape call `0x41e7ff` pushes `bl = 1`;
//! * `kind_of` merges at the definition level: `0x477b70` deep-copies the
//!   parent's parsed definition before `0x479240` parses the child's own
//!   record over it (`0x474c25`/`0x474c82`), so the retail `wingman`
//!   definition holds `basic_airplane`'s `mode_alt`/`collision_d` words.

use cs_app::mission_ai::{
    M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_SPAWN_WORDS, PilotInputGains, RUDDER_TOL, StickWords,
};
use cs_content::coordinates::load_engine_image;
use cs_content::stunts::ZrdValue;

use crate::campaign::m01_wingman_gains::{
    bytes_at, member_document, record_field, text_at, vehicle_records,
};
use crate::campaign::vs_m01_rt_window::game_dir;

/// The constants the crate flies are the merged `kind_of` words — and a
/// re-tuned constant cannot quietly pass, because the production scaling
/// the hillside flies is exercised here on the real code path.
#[test]
fn accept_m01_wingman_callsite_gains_are_the_merged_kind_of_words() {
    assert_eq!(
        M01_WINGMAN_PILOT_GAINS,
        PilotInputGains {
            scale: [0.6, 0.0, 0.15],
            limit: [100.0, 100.0, 0.25],
        }
    );
    assert_eq!(M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_SPAWN_WORDS);
    assert_eq!(RUDDER_TOL, 0.2);

    // Step 7 (`StickWords::scaled`) on the production constant: the
    // inherited `mode_alt` 0.0 zeroes the pitch axis outright.
    let words = StickWords {
        a100: 0.5,
        a108: -0.5,
        a10c: 1.0,
        recover_throttle: false,
    };
    let scaled = words.scaled(M01_WINGMAN_PILOT_GAINS);
    assert_eq!(scaled.a100, 0.3, "0.5 * 0.6");
    assert_eq!(scaled.a108, 0.0, "-0.5 * 0.0");
    assert_eq!(scaled.a10c, 0.15, "1.0 * 0.15");
    let input = words.flight_input(0.5);
    assert_eq!(input.pitch, 0.0, "no pitch authority reaches the input");
    assert!(input.roll < 0.0, "a100 negated onto the roll channel");
    assert!(input.yaw > 0.0);
}

/// The `e8 rel32` call at `site` resolves to `target` in the owner image.
fn call_rel32_is(bytes: &[u8], site: u64, target: u64) {
    let window = bytes_at(bytes, site, 5);
    assert_eq!(window[0], 0xe8, "a direct call at {site:#x}");
    let rel = i32::from_le_bytes(window[1..5].try_into().expect("rel32"));
    assert_eq!(
        (site + 5).wrapping_add(rel as i64 as u64),
        target,
        "the call at {site:#x} targets {target:#x}"
    );
}

/// **The mode-4 dispatch and the station call's `bl = 0`.** Every byte
/// window the call-site answer rests on: the `0x41c270` mode dispatch, the
/// sub-state gates inside `0x41e760`, both `0x41b560` sites'
/// fourth-argument pushes, the two phase jump tables and the law's `bl`
/// gate.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_callsite_dispatch_windows_select_the_station_call() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // All seven `0x41b560` call sites in the image.
    for site in [
        0x41d301_u64,
        0x41d4e0,
        0x41dab7,
        0x41e2e4,
        0x41e7ff,
        0x41eed3,
        0x42176a,
    ] {
        call_rel32_is(bytes, site, 0x41b560);
    }

    // `0x41c270`: `[esi+0x67c] == 4` jumps to `0x41c358` → `call 0x41e760`.
    assert_eq!(
        bytes_at(bytes, 0x41c2dd, 9),
        &[0x83, 0xbe, 0x7c, 0x06, 0x00, 0x00, 0x04, 0x74, 0x72],
        "cmp dword [esi+0x67c], 4 / je 0x41c358"
    );
    assert_eq!(
        bytes_at(bytes, 0x41c358, 2),
        &[0x56, 0xe8],
        "push esi / call …"
    );
    call_rel32_is(bytes, 0x41c359, 0x41e760);

    // Inside `0x41e760`: the follower sub-state `[esi+0x358]` gates first —
    // `!= 3` skips the escape climb, `== 4` (stunned) returns without
    // calling the law at all.
    assert_eq!(
        bytes_at(bytes, 0x41e7bd, 11),
        &[
            0x8b, 0x86, 0x58, 0x03, 0x00, 0x00, 0x83, 0xf8, 0x03, 0x75, 0x4d,
        ],
        "mov eax, [esi+0x358] / cmp eax, 3 / jne 0x41e815"
    );
    assert_eq!(
        bytes_at(bytes, 0x41e815, 9),
        &[0x83, 0xf8, 0x04, 0x0f, 0x84, 0xdb, 0x07, 0x00, 0x00],
        "cmp eax, 4 / je 0x41eff9 (stunned: no call)"
    );

    // The escape climb (`+0x358 == 3`): `push 0` then `push 1` — `bl = 1`
    // selects the `ai_emerg_input_*` set; goal = own position + 1000.0 up.
    assert_eq!(
        bytes_at(bytes, 0x41e7d6, 4),
        &[0x6a, 0x00, 0x6a, 0x01],
        "push 0 / push 1 — the escape call's 4th argument is 1"
    );
    assert_eq!(
        bytes_at(bytes, 0x41e7e6, 5),
        &[0x68, 0x28, 0xfb, 0x61, 0x00],
        "push 0x61fb28 — the follower's shared params"
    );
    call_rel32_is(bytes, 0x41e7ff, 0x41b560);

    // The phase machine on `[esi+0xd8]`: two 5-entry jump tables; every
    // body recomputes `[esi+0xe0]` and falls into the same station call.
    assert_eq!(
        bytes_at(bytes, 0x41ea8f, 7),
        &[0x0f, 0x87, 0x3d, 0x03, 0x00, 0x00, 0xff],
        "ja 0x41edd2 / jmp …"
    );
    assert_eq!(
        bytes_at(bytes, 0x41ea95, 7),
        &[0xff, 0x24, 0x85, 0x20, 0xf0, 0x41, 0x00],
        "jmp dword [eax*4 + 0x41f020]"
    );
    let u32s_at = |va: u64, n: usize| -> Vec<u32> {
        bytes_at(bytes, va, 4 * n)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect()
    };
    assert_eq!(
        u32s_at(0x41f00c, 5),
        vec![0x41e834, 0x41ea6c, 0x41e8bb, 0x41e9ec, 0x41ea2d],
        "the [esi+0xd8] phase table"
    );
    assert_eq!(
        u32s_at(0x41f020, 5),
        vec![0x41ea9c, 0x41ebc9, 0x41ec5a, 0x41edd2, 0x41ed35],
        "the re-dispatched phase table, all converging at 0x41edd2"
    );

    // The station call `0x41eed3`: `push 0` then `push 0` — `bl = 0`, the
    // normal `vehicle+0x8c4..+0x8d8` set — goal `[esi+0xe0]`, the
    // leader-relative station point.
    assert_eq!(
        bytes_at(bytes, 0x41eebd, 9),
        &[0x6a, 0x00, 0x6a, 0x00, 0x68, 0x28, 0xfb, 0x61, 0x00],
        "push 0 / push 0 / push 0x61fb28 — the station call's 4th argument is 0"
    );
    assert_eq!(
        bytes_at(bytes, 0x41eeca, 6),
        &[0x8d, 0x86, 0xe0, 0x00, 0x00, 0x00],
        "lea eax, [esi+0xe0] — the computed station goal"
    );
    call_rel32_is(bytes, 0x41eed3, 0x41b560);

    // The law's fourth-argument gate this selects through (`bl == 0` skips
    // the `+0x8dc..+0x8f0` emergency loads).
    assert_eq!(
        bytes_at(bytes, 0x41be47, 4),
        &[0x84, 0xdb, 0x74, 0x08],
        "test bl, bl / je over the emergency scale load"
    );
}

/// **`kind_of` merges the parent's parsed definition.** The registry
/// loader's `kind_of` lookup, the deep-copy constructor `0x477b70`'s field
/// copies (parent `ebx` → child `ebp`), and the child's own record parse
/// `0x479240` that runs after the copy.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_callsite_kind_of_copy_copies_the_parent_words() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The loader looks `kind_of` up in the record's own node (`0x57a090`),
    // name-matches the parent in the already-registered definitions and
    // calls `0x477b70(this, name, parent)`; on a miss it falls back to the
    // plain constructor `0x478a00`. The child's own record is then parsed
    // by `0x479240`, and the finished definition registers at `0x71daa8`.
    assert_eq!(
        bytes_at(bytes, 0x474ba5, 5),
        &[0x68, 0x24, 0x7b, 0x62, 0x00],
        "push \"kind_of\""
    );
    assert_eq!(text_at(bytes, 0x627b24), "kind_of");
    call_rel32_is(bytes, 0x474bbb, 0x57a090);
    assert_eq!(
        bytes_at(bytes, 0x474bc5, 2),
        &[0x74, 0x73],
        "je 0x474c3a — no kind_of → the plain constructor"
    );
    call_rel32_is(bytes, 0x474c25, 0x477b70);
    call_rel32_is(bytes, 0x474c82, 0x479240);

    // `0x477b70` copies the parent's scalar fields into the child —
    // `mov r32, [ebx+off]` (parent) then `mov [ebp+off], r32` (child) —
    // including the six spawn words and the `rudder_tol`/`ai_input_*`/
    // `ai_emerg_input_*` block.
    for (read_va, write_va, offset) in [
        (0x477fd6_u64, 0x477fdc_u64, 0xa4_u64),
        (0x477fe2, 0x477fe8, 0xa8),
        (0x477fee, 0x477ff4, 0xac),
        (0x477ffa, 0x478000, 0xb0),
        (0x478012, 0x478018, 0xb4),
        (0x478006, 0x47800c, 0xb8),
        (0x47801e, 0x478024, 0xbc),
        (0x4788dd, 0x4788e3, 0x260),
        (0x478948, 0x47894e, 0x264),
        (0x47893c, 0x478942, 0x268),
        (0x478954, 0x47895a, 0x26c),
        (0x47896c, 0x478972, 0x270),
        (0x478960, 0x478966, 0x274),
        (0x478978, 0x47897e, 0x278),
        (0x4789cc, 0x4789d2, 0x294),
    ] {
        let read = bytes_at(bytes, read_va, 6);
        assert_eq!(read[0], 0x8b, "a parent load at {read_va:#x}");
        assert_eq!(
            u32::from_le_bytes(read[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the parent read at {read_va:#x} names def+{offset:#x}"
        );
        let write = bytes_at(bytes, write_va, 6);
        assert_eq!(write[0], 0x89, "a child store at {write_va:#x}");
        assert_eq!(
            u32::from_le_bytes(write[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the child store at {write_va:#x} names def+{offset:#x}"
        );
    }
    // The 9-dword block at `+0x23c` is copied with a single rep movsd.
    assert_eq!(
        bytes_at(bytes, 0x478935, 7),
        &[0xb9, 0x09, 0x00, 0x00, 0x00, 0xf3, 0xa5],
        "mov ecx, 9 / rep movsd at 0x47893a"
    );
}

/// The merged-view helper: `record`'s `kind_of` parent name, if declared.
fn kind_of(record: &ZrdValue) -> Option<&str> {
    record_field(record, "kind_of")?
        .as_list()?
        .first()?
        .as_text()
}

/// The value of `key` in `record`'s merged view — the record's own key when
/// it declares one, else the nearest `kind_of` ancestor's, mirroring the
/// `0x477b70` copy-then-parse order (parent must be registered before the
/// child parses).
fn merged_field<'a>(
    records: &[(&'a str, &'a ZrdValue)],
    record: &'a ZrdValue,
    key: &str,
) -> Option<&'a ZrdValue> {
    let mut current = record;
    let mut depth = 0;
    loop {
        if let Some(value) = record_field(current, key) {
            return Some(value);
        }
        let parent = kind_of(current)?;
        current = records
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(parent))
            .map(|(_, record)| *record)
            .unwrap_or_else(|| panic!("kind_of parent {parent} is registered"));
        depth += 1;
        assert!(depth < 8, "the kind_of chain does not cycle");
    }
}

fn floats(value: &ZrdValue) -> Vec<f32> {
    value
        .as_list()
        .expect("a float-list value")
        .iter()
        .map(|node| match node {
            ZrdValue::Float(number) => *number,
            other => panic!("a float-list element: {other:?}"),
        })
        .collect()
}

/// **The retail data through the merge.** `vehicle.zrd` lists the records
/// in registration order, `wingman`'s `kind_of` chain is
/// `wingman` → `devastator` → `basic_airplane`, and the merged view holds
/// `basic_airplane`'s `mode_alt`/`collision_d`/`ai_input_*` words — the
/// spawn copy's first source then gives the scale `[0.6, 0.0, 0.15]` and
/// limits `[100, 100, 0.25]` the crate flies.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_callsite_retail_merges_basic_airplane_words() {
    let root = game_dir();
    let (vehicles, _span) = member_document(&root, "zbd/zrdr.zbd", "vehicle.zrd");
    let records = vehicle_records(&vehicles);

    // Registration order: each `kind_of` parent precedes its child, which
    // is what `0x477b70`'s already-parsed parent copy needs.
    let index_of = |name: &str| {
        records
            .iter()
            .position(|(record, _)| name.eq_ignore_ascii_case(record))
            .unwrap_or_else(|| panic!("vehicle.zrd declares {name}"))
    };
    let (basic, devastator, wingman) = (
        index_of("basic_airplane"),
        index_of("devastator"),
        index_of("wingman"),
    );
    assert!(basic < devastator && devastator < wingman);

    let wingman_rec = records[wingman].1;
    let devastator_rec = records[devastator].1;
    assert_eq!(kind_of(wingman_rec), Some("devastator"));
    assert_eq!(kind_of(devastator_rec), Some("basic_airplane"));
    assert_eq!(kind_of(records[basic].1), None);

    // The merged view for the words the spawn copy reads first.
    assert_eq!(
        floats(merged_field(&records, wingman_rec, "mode_alt").expect("mode_alt")),
        [0.0]
    );
    assert_eq!(
        floats(merged_field(&records, wingman_rec, "collision_d").expect("collision_d")),
        [0.6, 0.15]
    );
    for key in [
        "ai_input_scale_pitch",
        "ai_input_scale_roll",
        "ai_input_scale_yaw",
    ] {
        assert_eq!(
            floats(merged_field(&records, wingman_rec, key).expect(key)),
            [3.5],
            "{key} merges from basic_airplane"
        );
    }
    for key in [
        "ai_input_limit_pitch",
        "ai_input_limit_roll",
        "ai_input_limit_yaw",
    ] {
        assert_eq!(
            floats(merged_field(&records, wingman_rec, key).expect(key)),
            [1.0],
            "{key} merges from basic_airplane"
        );
    }
    // No record in the chain declares `rudder_tol`: the definition keeps
    // the constructor's `def+0x260` = 0.2.
    assert!(merged_field(&records, wingman_rec, "rudder_tol").is_none());

    // The merged definition's six words `+0xa8..+0xbc` — `mode_alt`,
    // `collision_d`'s two components, then the constructor's three limit
    // words — mapped to gains the way the spawn copy maps them (pitch,
    // roll, yaw for both scale and limit).
    let def = [0.0_f32, 0.6, 0.15, 100.0, 100.0, 0.25];
    let gains = PilotInputGains {
        scale: [def[1], def[0], def[2]],
        limit: [def[4], def[3], def[5]],
    };
    assert_eq!(gains.scale, [0.6, 0.0, 0.15]);
    assert_eq!(gains, M01_WINGMAN_PILOT_GAINS);
    assert_eq!(RUDDER_TOL, 0.2);
}
