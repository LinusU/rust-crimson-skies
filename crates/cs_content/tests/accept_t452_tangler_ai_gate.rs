//! Task T452-REG-TANGLER-AI-GATE (#2202): what the tangle branch at `0x4202e3`
//! skips. Task test prefix: `accept_t452_tangler_`.
//!
//! Static measurement only: the committed `[[tangler_fact]]` rows in
//! `data/owner-static/engine-facts.toml` carry the addresses and bytes of every
//! cited site, this module checks the rows against each other, and the ignored
//! image test re-reads every window from `$CS_ENGINE_IMAGE`. Finding:
//! `docs/findings/2026-10-29-t452-tangler-ai-gate.md`. Nothing here is
//! original-run evidence and no test awards `verified_original`.
//!
//! The measured shape: `0x4201a0` is the actor's maneuver chooser. It loops the
//! seventeen `maneuvers.zrd` records at `0x71b210` (stride `0x1c`) and, for each
//! one whose gate passes, builds a scored candidate and inserts it into a
//! weighted random draw. The gate at `0x4202ca`/`0x4202e3` skips a maneuver whose
//! record byte `+5` (its `nitro` key) is set when the actor has no nitrous
//! injector (`+0x946`) or is tangled (status word `+0x2dc`, flag 2).

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// The maneuver array the chooser loops: `0x11` records of `0x1c` bytes, filled
/// from `maneuvers.zrd` by `0x45f340`.
const MANEUVER_TABLE: u64 = 0x71_b210;
const MANEUVER_STRIDE: u64 = 0x1c;
const MANEUVER_COUNT: usize = 0x11;

/// The record constructor every `maneuvers.zrd` name is handed to.
const MANEUVER_CONSTRUCTOR: i64 = 0x45_f3e0;

/// The chooser whose gates are measured here.
const CHOPPER: i64 = 0x42_01a0;

/// The three direct callers of the chooser, in address order.
const CHOPPER_CALLERS: [u64; 3] = [0x41_db9d, 0x43_e10e, 0x4b_9ff8];

/// `(name, va, bytes, fact)` of every committed `[[tangler_fact]]` row this task
/// added.
fn rows() -> Vec<(String, u64, Vec<u8>, String)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a tangler_fact row lacks {name}"))
    };
    TABLE
        .split("[[tangler_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .filter_map(|chunk| {
            let name = field(chunk, "name");
            if !name.starts_with("tangler_ai.") {
                return None;
            }
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            assert_eq!(field(chunk, "provenance"), "static");
            Some((name, va, bytes, field(chunk, "fact")))
        })
        .collect()
}

/// The window of one row, by name.
fn row_bytes<'a>(rows: &'a [(String, u64, Vec<u8>, String)], name: &str) -> &'a [u8] {
    rows.iter()
        .find(|row| row.0 == name)
        .unwrap_or_else(|| panic!("the {name} row"))
        .2
        .as_slice()
}

/// The closed inventory of this stage's rows, in the order the table holds them,
/// with the address each window was measured at, and the bytes of the gate
/// instruction by instruction.
#[test]
fn accept_t452_tangler_ai_gate_rows_name_the_branch_and_the_fields_it_tests() {
    let rows = rows();
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "tangler_ai.gate_sequence",
            "tangler_ai.skip_target",
            "tangler_ai.record_base",
            "tangler_ai.record_stride",
            "tangler_ai.explicit_index",
            "tangler_ai.nitro_key",
            "tangler_ai.maneuver_table",
            "tangler_ai.maneuver_names",
            "tangler_ai.boost_on_choice",
            "tangler_ai.weighted_pick",
        ]
    );
    let vas: Vec<u64> = rows.iter().map(|r| r.1).collect();
    assert_eq!(
        vas,
        [
            0x42_02ca, 0x42_0695, 0x42_0227, 0x42_0301, 0x42_01f5, 0x45_f4db, 0x45_f340, 0x45_f60c,
            0x42_0919, 0x42_0723,
        ]
    );

    // The gate, instruction by instruction: the maneuver's `nitro` byte decides
    // whether the gate is consulted at all, then the injector flag and the
    // tangle flag both take the same branch to the skip target.
    let gate = row_bytes(&rows, "tangler_ai.gate_sequence");
    assert_eq!(gate.len(), 38);
    assert_eq!(gate[..7], [0x8a, 0x0c, 0x95, 0x15, 0xb2, 0x71, 0x00]); // mov cl,[edx*4+0x71b215]
    assert_eq!(gate[7..11], [0x84, 0xc9, 0x74, 0x1b]); // test cl,cl / je 0x4202f0
    assert_eq!(gate[11..17], [0x8a, 0x8e, 0x46, 0x09, 0x00, 0x00]); // mov cl,[esi+0x946]
    assert_eq!(
        gate[17..25],
        [0x84, 0xc9, 0x0f, 0x84, 0xb2, 0x03, 0x00, 0x00]
    ); // je 0x420695
    assert_eq!(
        gate[25..],
        [
            0xf6, 0x86, 0xdc, 0x02, 0x00, 0x00, 0x02, 0x0f, 0x85, 0xa5, 0x03, 0x00, 0x00
        ]
    ); // test byte [esi+0x2dc],2 / jne 0x420695

    // The skip target is the loop tail, not an action: increment, bounds, back.
    let skip = row_bytes(&rows, "tangler_ai.skip_target");
    assert_eq!(skip[..4], [0xc6, 0x45, 0xfc, 0x00]); // mov byte [ebp-4],0
    assert_eq!(skip[4], 0x43); // inc ebx
    assert_eq!(skip[5..8], [0x83, 0xfb, 0x11]); // cmp ebx,0x11
    assert_eq!(skip[11..], [0x0f, 0x8c, 0x3b, 0xfb, 0xff, 0xff]); // jl 0x4201e1

    // Byte `+5` is a presence flag read from the maneuver's `nitro` key.
    let nitro = row_bytes(&rows, "tangler_ai.nitro_key");
    assert_eq!(nitro[..5], [0x68, 0x9c, 0x58, 0x62, 0x00]); // push 0x62589c
    assert_eq!(nitro[18..22], [0xc6, 0x46, 0x05, 0x01]); // mov byte [esi+5],1

    // The chosen maneuver enables the injector through the routine a tangle
    // refuses, and the draw is weighted by `rand()/32767.7`.
    let boost = row_bytes(&rows, "tangler_ai.boost_on_choice");
    assert_eq!(boost[..7], [0x8a, 0x04, 0x95, 0x15, 0xb2, 0x71, 0x00]); // mov al,[eax*4+0x71b215]
    assert_eq!(
        boost[9..20],
        [
            0x74, 0x09, 0x6a, 0x01, 0x8b, 0xcf, 0xe8, 0xe3, 0x17, 0x09, 0x00
        ]
    ); // je / push 1 / mov ecx,edi / call 0x4b2110
    let pick = row_bytes(&rows, "tangler_ai.weighted_pick");
    assert_eq!(pick[..6], [0xff, 0x15, 0x50, 0x03, 0xa2, 0x00]); // call [0xa20350]
    assert_eq!(pick[20..26], [0xd8, 0x0d, 0x98, 0x35, 0x60, 0x00]); // fmul [0x603598]

    // A window with no sentence naming the site is not a measurement.
    for (name, _, _, fact) in &rows {
        assert!(fact.len() > 80, "{name} states what the window shows");
        assert!(
            fact.matches("0x").count() >= 4,
            "{name} cites the addresses its window is about"
        );
    }
}

/// The names row is the closed proof that the chooser loops seventeen
/// `maneuvers.zrd` maneuvers: seventeen calls to the record constructor, in
/// order, each preceded by its record slot and its name string.
#[test]
fn accept_t452_tangler_ai_gate_the_names_row_names_seventeen_maneuvers() {
    let rows = rows();
    let row = rows
        .iter()
        .find(|row| row.0 == "tangler_ai.maneuver_names")
        .expect("the names row");
    let (va, bytes) = (row.1, row.2.as_slice());

    let mut calls = Vec::new();
    for i in 0..=bytes.len() - 5 {
        if bytes[i] != 0xe8 {
            continue;
        }
        let rel = i32::from_le_bytes(bytes[i + 1..i + 5].try_into().unwrap());
        if va as i64 + i as i64 + 5 + i64::from(rel) == MANEUVER_CONSTRUCTOR {
            calls.push(i);
        }
    }
    assert_eq!(
        calls.len(),
        MANEUVER_COUNT,
        "one constructor call per maneuver"
    );

    let mut slots = Vec::new();
    let mut names = Vec::new();
    for i in &calls {
        // push <name> (5 bytes), push esi (1), mov ecx,<slot> (5), call (5).
        assert_eq!(bytes[i - 11], 0x68, "push imm32 of the maneuver name");
        assert_eq!(bytes[i - 6], 0x56, "push esi: the maneuvers.zrd file");
        assert_eq!(bytes[i - 5], 0xb9, "mov ecx,imm32 of the record");
        names.push(u64::from(u32::from_le_bytes(
            bytes[i - 10..i - 6].try_into().unwrap(),
        )));
        slots.push(u64::from(u32::from_le_bytes(
            bytes[i - 4..*i].try_into().unwrap(),
        )));
    }

    let expected_slots: Vec<u64> = (0..MANEUVER_COUNT as u64)
        .map(|i| MANEUVER_TABLE + i * MANEUVER_STRIDE)
        .collect();
    assert_eq!(slots, expected_slots, "0x71b210 + 0x1c * index");
    assert_eq!(
        names,
        [
            0x62_58d0, 0x62_58d8, 0x62_58e4, 0x62_58f0, 0x62_58f8, 0x62_5900, 0x62_5908, 0x62_5914,
            0x62_5928, 0x62_5934, 0x62_5940, 0x62_5948, 0x62_5950, 0x62_5964, 0x62_5970, 0x62_597c,
            0x62_5988,
        ],
        "the maneuver name strings, in the order the records are constructed"
    );

    // The array is exactly those seventeen records: the last ends on the guard
    // byte the loader tests before it fills them.
    assert_eq!(
        MANEUVER_TABLE + MANEUVER_COUNT as u64 * MANEUVER_STRIDE,
        0x71_b3ec
    );

    // The array constructor's own arguments agree.
    let table = row_bytes(&rows, "tangler_ai.maneuver_table");
    assert_eq!(table[10..14], [0x6a, 0x11, 0x6a, 0x1c]); // push 0x11 / push 0x1c
    assert_eq!(table[14..19], [0x68, 0x10, 0xb2, 0x71, 0x00]); // push 0x71b210
}

/// Every cited byte window is in the owner's decrypted executable, the status
/// test this stage explains is the one `tangler.ai_gate` cited, the index of the
/// chooser's callers is closed, and the strings the gate and the loader key on
/// are the measured ones.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_tangler_ai_gate_image_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    let at = |va: u64, len: usize| -> &[u8] {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        &image[offset..offset + len]
    };

    let rows = rows();
    assert_eq!(rows.len(), 10);
    for (name, va, bytes, _) in &rows {
        assert_eq!(at(*va, bytes.len()), bytes.as_slice(), "{name} at 0x{va:x}");
    }

    // The status test this stage explains is the one `tangler.ai_gate` cited.
    assert_eq!(
        at(0x42_02e3, 11),
        &[
            0xf6, 0x86, 0xdc, 0x02, 0x00, 0x00, 0x02, 0x0f, 0x85, 0xa5, 0x03
        ][..]
    );

    // Closed inventory of direct calls to the chooser, with the eight bytes each
    // caller pushes and moves: `push -1` is the gated all-maneuver path, a
    // register push is the named-maneuver path that bypasses both gates.
    let mut callers = Vec::new();
    for offset in 0x1000..image.len().min(0x20_2000) - 5 {
        if image[offset] != 0xe8 {
            continue;
        }
        let rel = i32::from_le_bytes(image[offset + 1..offset + 5].try_into().unwrap());
        let va = offset as i64 + IMAGE_BASE as i64;
        if va + 5 + i64::from(rel) == CHOPPER {
            callers.push((va as u64, image[offset - 8..offset].to_vec()));
        }
    }
    assert_eq!(
        callers.iter().map(|c| c.0).collect::<Vec<_>>(),
        CHOPPER_CALLERS
    );
    // `push -1; mov ecx, esi` before the gated callers, `push eax; mov ecx, esi`
    // before the named-maneuver one.
    assert_eq!(callers[0].1[4..], [0x6a, 0xff, 0x8b, 0xce]);
    assert_eq!(callers[2].1[4..], [0x6a, 0xff, 0x8b, 0xce]);
    assert_eq!(callers[1].1[5..], [0x50, 0x8b, 0xce]);

    // The keys: the loader's file, the gate's record byte, the console hint.
    assert_eq!(at(0x62_58c0, 14), b"maneuvers.zrd\0".as_slice());
    assert_eq!(at(0x62_589c, 6), b"nitro\0".as_slice());
    assert_eq!(
        at(0x62_2df8, 41),
        b"Maneuver number must be between 0 and %d.".as_slice()
    );
}
