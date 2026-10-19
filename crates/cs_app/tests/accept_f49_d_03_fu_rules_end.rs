//! F49-D.03-FU-RULES-END (#1638): the original Instant Action end condition,
//! respawns and deadline per `mission_type`, bound into the retail catalog.
//!
//! The measurement (`docs/findings/2026-10-19-f49-d-03-fu-rules-end.md`) is the
//! executable's Instant Action end check `0x45b9d0`: one branch per mission
//! type, no deadline, no replacement budget, and a caller that cannot run the
//! check while the player is down. The engine-image tests re-read the bytes at
//! the named virtual addresses independently of the production code; the
//! retail test asserts every catalog preset's rules equal the mapping the
//! image implies; the synthetic tests pin the snapshot semantics the
//! measurement implies (dormant waves, the bound-object objective, the
//! player-down pre-emption).
//!
//! Required capabilities: **retail** (`$CS_GAME_DIR`) and `$CS_ENGINE_IMAGE`,
//! read-only. Nothing is run, rendered or played, so the claims stay
//! `implemented`, never `verified_original`.

use std::fs;
use std::path::PathBuf;

use cs_app::ui::instant_action::{
    DirectoryInputs, ScenarioResult, ScenarioSnapshot, directory_presets, evaluate_outcome,
    lower_preset, retail_instant_action_catalog,
};
use cs_content::ai::DifficultyTier;
use cs_content::instant_action::{
    Declared, IA_MISSION_TYPE_VOCABULARY, IA_RULES_END_CLAIM, InstantActionCatalog,
    InstantActionPreset, RespawnBudget, RosterSlot, ScenarioOptions, ScenarioParameters,
    ScenarioSchemaError, ScenarioSide, TieOutcome, VictoryCondition, VictoryRules,
    measured_ia_victory_rules, read_ia_descriptor, synthetic_difficulty,
    synthetic_instant_action_catalog, synthetic_scenario_options,
};
use cs_content::stunts::decode_zrd;
use cs_content::world::WorldId;
use cs_types::content::ContentId;

const FINDING: &str = "docs/findings/2026-10-19-f49-d-03-fu-rules-end.md";

// ------------------------------------------------------- engine image --------

/// The image's bytes at virtual address `va`, through its PE section table.
fn image_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let u16_at = |at: usize| u16::from_le_bytes(image[at..at + 2].try_into().expect("2 bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0", "a PE image");
    let sections = usize::from(u16_at(pe + 6));
    let optional = usize::from(u16_at(pe + 20));
    let image_base = u32_at(pe + 24 + 28);
    let table = pe + 24 + optional;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8);
        let section_va = image_base + u32_at(entry + 12);
        let raw_size = u32_at(entry + 16);
        let raw = u32_at(entry + 20) as usize;
        if va >= section_va && va - section_va + len as u32 <= virtual_size.min(raw_size) {
            let start = raw + (va - section_va) as usize;
            return &image[start..start + len];
        }
    }
    panic!("virtual address {va:#x} is in no section");
}

fn image_u32(image: &[u8], va: u32) -> u32 {
    u32::from_le_bytes(
        image_bytes(image, va, 4)
            .try_into()
            .expect("a dword is 4 bytes"),
    )
}

/// The NUL-terminated text at `va`, if it is printable ASCII.
fn c_string(image: &[u8], va: u32) -> Option<String> {
    let mut text = String::new();
    for offset in 0..64 {
        let byte = *image_bytes(image, va + offset, 1).first()?;
        match byte {
            0 => return Some(text),
            0x20..=0x7e => text.push(char::from(byte)),
            _ => return None,
        }
    }
    None
}

fn engine_image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    fs::read(path).expect("the engine image reads")
}

/// The parser `0x458c20` compares `ia.zrd`'s `mission_type` case-insensitively
/// against five strings; each name maps to the index the end check dispatches
/// on, and anything else returns the sentinel 5.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_end_image_the_type_names_parse_to_the_bound_conditions() {
    let image = engine_image();

    // The compare is _stricmp through the import cell.
    assert_eq!(
        image_bytes(&image, 0x0045_8c27, 6),
        [0x8b, 0x3d, 0x14, 0x02, 0xa2, 0x00],
        "mov edi, [0xa20214]: the case-insensitive compare"
    );
    // The sentinel: unrecognised names return ebx = 5.
    assert_eq!(
        image_bytes(&image, 0x0045_8c33, 5),
        [0xbb, 5, 0, 0, 0],
        "mov ebx, 5: the sentinel index"
    );

    // (push VA, name string VA, name, produced index). `dogfight_ace` returns
    // the stricmp result itself (0 on a match); the rest `mov eax, index`.
    let cases: [(u32, u32, &str, u32); 5] = [
        (0x0045_8c2d, 0x0062_53dc, "dogfight_ace", 0),
        (0x0045_8c45, 0x0062_53ec, "dogfight_squadron", 1),
        (0x0045_8c5d, 0x0062_5400, "zeppelin_run", 2),
        (0x0045_8c75, 0x0062_5410, "ground_target", 3),
        (0x0045_8c8d, 0x0062_5420, "stunt_flying", 4),
    ];
    for (push_va, string_va, name, index) in cases {
        let mut push_bytes = vec![0x68u8];
        push_bytes.extend_from_slice(&string_va.to_le_bytes());
        assert_eq!(
            image_bytes(&image, push_va, 5),
            push_bytes.as_slice(),
            "push {name:?} at {push_va:#x}"
        );
        assert_eq!(c_string(&image, string_va).as_deref(), Some(name));
        // The index the production binding implies must be the index the
        // engine image produces — asserted through the per-type branch the
        // end check dispatches on (below) and the mapping itself.
        assert_eq!(
            measured_ia_victory_rules(name).map(|rules| rules.condition()),
            Some(expected_condition(index)),
            "the production binding for {name} is the measured type-{index} condition"
        );
    }
    // Index producers 1..=4 are explicit `mov eax, index` (7 bytes: pop, pop,
    // mov — the `pop edi; pop esi` pair precedes each mov).
    for (va, index) in [
        (0x0045_8c56u32, 1u8),
        (0x0045_8c6e, 2),
        (0x0045_8c86, 3),
        (0x0045_8c9a, 4),
    ] {
        assert_eq!(
            image_bytes(&image, va, 5),
            [0xb8, index, 0, 0, 0],
            "mov eax, {index} at {va:#x}"
        );
    }
    // A name matching none returns the sentinel.
    assert_eq!(
        image_bytes(&image, 0x0045_8ca1, 2),
        [0x8b, 0xc3],
        "mov eax, ebx: the sentinel is the miss result"
    );

    // The scenario reader stores the parsed index only when it is not the
    // sentinel; the record was initialised to 0 (dogfight_ace).
    assert_eq!(
        image_bytes(&image, 0x0045_93c9, 5),
        [0xbf, 5, 0, 0, 0],
        "mov edi, 5: the sentinel the store compares against"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_93d7, 5),
        [0xe8, 0x44, 0xf8, 0xff, 0xff],
        "call 0x458c20: the type-name parser"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_93df, 4),
        [0x3b, 0xc7, 0x74, 0x02],
        "cmp eax, edi; je: a sentinel leaves the record alone"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_8ffa, 7),
        [0xc7, 0x45, 0x00, 0, 0, 0, 0],
        "mov [ebp], 0: the record's initialised mission type is 0"
    );

    // `measured_ia_victory_rules` covers exactly the engine's vocabulary.
    assert_eq!(IA_MISSION_TYPE_VOCABULARY.len(), 5);
    for name in IA_MISSION_TYPE_VOCABULARY {
        assert!(
            measured_ia_victory_rules(name).is_some(),
            "{name} is in the surveyed vocabulary and must bind"
        );
    }
    assert!(
        measured_ia_victory_rules("capture_the_flag").is_none(),
        "a name outside the measured vocabulary binds nothing"
    );
}

/// The Instant Action menu maps selection 0..3 onto mission types 0, 1, 4, 2 —
/// no menu row produces `ground_target` (3), which no retail directory offers.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_end_image_the_menu_writes_types_0_1_4_2() {
    let image = engine_image();
    assert_eq!(
        image_bytes(&image, 0x0041_754d, 7),
        [0xff, 0x24, 0x85, 0xd8, 0x75, 0x41, 0x00],
        "jmp [eax*4 + 0x4175d8]: the selection jump table"
    );
    // The four cases write the live mission type global [0x718cd8].
    for (index, write_va, mission_type) in [
        (0u32, 0x0041_7554u32, 0u8),
        (1, 0x0041_7560, 1),
        (2, 0x0041_756c, 4),
        (3, 0x0041_7578, 2),
    ] {
        assert_eq!(
            image_u32(&image, 0x0041_75d8 + index * 4),
            write_va,
            "menu selection {index} reaches {write_va:#x}"
        );
        assert_eq!(
            image_bytes(&image, write_va, 10),
            [0xc7, 0x05, 0xd8, 0x8c, 0x71, 0x00, mission_type, 0, 0, 0],
            "menu selection {index} writes mission type {mission_type}"
        );
    }
}

/// The measured byte predicates of the end check `0x45b9d0`, its per-type
/// tail, the win path, the caller's down gate — and the negatives the binding
/// records (no clock read, exactly one `0x4b0f40` call and it is a wake).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_end_image_the_end_check_predicates() {
    let image = engine_image();
    let assert_bytes = |va: u32, bytes: &[u8], what: &str| {
        assert_eq!(image_bytes(&image, va, bytes.len()), bytes, "{what}");
    };

    // The dispatch: the live type is [0x718cd8]; type 0 takes its own branch.
    assert_bytes(
        0x0045_b9e9,
        &[0x8b, 0x1d, 0xd8, 0x8c, 0x71, 0x00],
        "mov ebx, [0x718cd8]",
    );
    assert_bytes(
        0x0045_b9fb,
        &[0x75, 0x40],
        "jne 0x45ba3d: nonzero types take the shared loop",
    );

    // Type 0 `dogfight_ace` (0x45b9fd): an opposing vehicle is settled when
    // +0x91d is set and +0x91f is clear; +0x945 is never read in this branch.
    assert_bytes(
        0x0045_ba0d,
        &[0x8b, 0x41, 0x08],
        "mov eax, [ecx+8]: the node holds the vehicle",
    );
    assert_bytes(
        0x0045_ba10,
        &[0x83, 0x78, 0x08, 0x01, 0x7e, 0x1c],
        "cmp [eax+8], 1; jle: only the opposing side counts",
    );
    assert_bytes(
        0x0045_ba16,
        &[0x8a, 0x98, 0x1d, 0x09, 0x00, 0x00],
        "mov bl, [eax+0x91d]",
    );
    assert_bytes(
        0x0045_ba1e,
        &[0x0f, 0x84, 0x39, 0x04, 0x00, 0x00],
        "je 0x45be5d: a not-down opponent blocks the win",
    );
    assert_bytes(
        0x0045_ba24,
        &[0x8a, 0x98, 0x1f, 0x09, 0x00, 0x00],
        "mov bl, [eax+0x91f]",
    );
    assert_bytes(
        0x0045_ba2c,
        &[0x0f, 0x85, 0x2b, 0x04, 0x00, 0x00],
        "jne 0x45be5d: a +0x91f-set opponent blocks the win",
    );
    assert_bytes(
        0x0045_ba38,
        &[0xe9, 0x03, 0x04, 0x00, 0x00],
        "jmp 0x45be40: all settled wins",
    );
    // The negative that distinguishes OppositionDown: the type-0 range reads
    // no +0x945 (0x000945 as a displacement).
    let type0 = image_bytes(&image, 0x0045_b9fd, (0x0045_ba3d - 0x0045_b9fd) as usize);
    assert!(
        !type0.windows(4).any(|w| w == [0x45, 0x09, 0x00, 0x00]),
        "the type-0 branch reads no +0x945 displacement"
    );

    // The shared loop (0x45ba3d): settled requires +0x945 clear (awake),
    // +0x91d set (down) and +0x91f clear; +0x388 is the wave group.
    for (va, byte) in [
        (0x0045_ba5c, 0x45u8),
        (0x0045_ba65, 0x1d),
        (0x0045_ba6e, 0x1f),
    ] {
        assert_bytes(
            va,
            &[0x80, 0xb8, byte, 0x09, 0x00, 0x00, 0x00],
            &format!("cmp byte [eax+0x9{byte:02x}], 0 at {va:#x}"),
        );
    }
    assert_bytes(
        0x0045_ba77,
        &[0x39, 0x90, 0x88, 0x03, 0x00, 0x00],
        "cmp [eax+0x388], edx: the wave group vs the counter",
    );
    assert_bytes(
        0x0045_ba82,
        &[0x0f, 0x84, 0xfb, 0x02, 0x00, 0x00],
        "je 0x45bd83: unsettled in the current wave ends the check",
    );

    // Wave advance: the counter increments at [0x718cd0]; a vehicle of the
    // new wave is woken with argument 0 (edi is cleared at 0x45b9f2), which
    // is activation of a declared member, not a replacement spawn.
    assert_bytes(
        0x0045_b9f2,
        &[0x33, 0xff],
        "xor edi, edi: the wake argument is 0",
    );
    assert_bytes(0x0045_ba9a, &[0x42], "inc edx");
    assert_bytes(
        0x0045_ba9e,
        &[0x89, 0x15, 0xd0, 0x8c, 0x71, 0x00],
        "mov [0x718cd0], edx: the wave counter advances",
    );
    assert_bytes(0x0045_bc14, &[0x57], "push edi: the wake argument");
    assert_bytes(
        0x0045_bc17,
        &[0xe8, 0x24, 0x53, 0x05, 0x00],
        "call 0x4b0f40: the sleep/wake setter",
    );

    // Negatives over the whole function: no read of the tick clock global
    // 0x71c470 (no deadline), and exactly one call to 0x4b0f40 — the wake —
    // so the check never sleeps or replaces an actor (respawns are None).
    let body = image_bytes(&image, 0x0045_b9d0, (0x0045_be65 - 0x0045_b9d0) as usize);
    assert!(
        !body.windows(4).any(|w| w == [0x70, 0xc4, 0x71, 0x00]),
        "the end check reads no 0x71c470 clock field"
    );
    let wake_calls: Vec<usize> = body
        .windows(5)
        .enumerate()
        .filter(|(_, w)| w[0] == 0xe8)
        .filter(|(at, w)| {
            let rel = i32::from_le_bytes(w[1..5].try_into().expect("rel32"));
            (0x0045_b9d0 + *at as u32 + 5).wrapping_add(rel as u32) == 0x004b_0f40
        })
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        wake_calls.len(),
        1,
        "the only 0x4b0f40 call inside the check is the wave wake"
    );

    // Type 3 `ground_target` tail: the bound object [0x718f50] wins when
    // absent or its vtable+0x14 call returns nonzero.
    assert_bytes(
        0x0045_bd83,
        &[0x83, 0x3d, 0xd8, 0x8c, 0x71, 0x00, 0x03],
        "cmp [0x718cd8], 3",
    );
    assert_bytes(
        0x0045_bd8c,
        &[0x8b, 0x0d, 0x50, 0x8f, 0x71, 0x00],
        "mov ecx, [0x718f50]: the bound ground target",
    );
    assert_bytes(0x0045_bd98, &[0xff, 0x50, 0x14], "call [eax+0x14]");
    // The spawn binds the wrapper at 0x45b1de; the wrapper ctor 0x4a6330
    // installs vtable 0x60886c, whose +0x14 entry is 0x4a5a70 — and that
    // entry returns the wrapped vehicle's +0x91d.
    assert_bytes(
        0x0045_b1de,
        &[0xa3, 0x50, 0x8f, 0x71, 0x00],
        "mov [0x718f50], eax: the target bind",
    );
    assert_bytes(
        0x004a_6344,
        &[0xc7, 0x00, 0x6c, 0x88, 0x60, 0x00],
        "mov [eax], 0x60886c: the wrapper vtable",
    );
    assert_eq!(
        image_u32(&image, 0x0060_8880),
        0x004a_5a70,
        "vtable 0x60886c entry +0x14"
    );
    assert_bytes(
        0x004a_5a70,
        &[0x8b, 0x41, 0x04, 0x8a, 0x80, 0x1d, 0x09, 0x00, 0x00, 0xc3],
        "the wrapper returns the wrapped vehicle's +0x91d",
    );

    // Type 4 `stunt_flying`: every gate record in [0x718f78, 0x718f7c) must
    // be consumed — +0x48 clear or +0x40 set; consumed gates clear the
    // marker record's +0x4d byte at [0x718f88][i].
    assert_bytes(
        0x0045_bda9,
        &[0x83, 0x3d, 0xd8, 0x8c, 0x71, 0x00, 0x04],
        "cmp [0x718cd8], 4",
    );
    assert_bytes(
        0x0045_bdb2,
        &[0x8b, 0x15, 0x78, 0x8f, 0x71, 0x00],
        "mov edx, [0x718f78]: the gate vector",
    );
    assert_bytes(0x0045_bdd5, &[0x8a, 0x59, 0x48], "mov bl, [ecx+0x48]");
    assert_bytes(0x0045_bddc, &[0x8a, 0x59, 0x40], "mov bl, [ecx+0x40]");
    assert_bytes(
        0x0045_bdeb,
        &[0x8b, 0x15, 0x88, 0x8f, 0x71, 0x00],
        "mov edx, [0x718f88]: the marker vector",
    );
    assert_bytes(
        0x0045_bdf4,
        &[0xc6, 0x41, 0x4d, 0x00],
        "mov byte [ecx+0x4d], 0: the consumed gate's marker",
    );

    // Type 2 `zeppelin_run`: the bound zeppelin object
    // 0x718fd0[4*[0x718f2c]] wins when absent, when its component list
    // [+0x4c, +0x50) is empty, or when its +0x06 byte is set.
    assert_bytes(
        0x0045_be01,
        &[0x83, 0x3d, 0xd8, 0x8c, 0x71, 0x00, 0x02],
        "cmp [0x718cd8], 2",
    );
    assert_bytes(
        0x0045_be0a,
        &[0x8b, 0x15, 0x2c, 0x8f, 0x71, 0x00],
        "mov edx, [0x718f2c]: the bound record index",
    );
    assert_bytes(
        0x0045_be10,
        &[0x8b, 0x04, 0x95, 0xd0, 0x8f, 0x71, 0x00],
        "mov eax, [edx*4 + 0x718fd0]: the bound zeppelin",
    );
    assert_bytes(
        0x0045_be1b,
        &[0x8b, 0x48, 0x4c],
        "mov ecx, [eax+0x4c]: component list begin",
    );
    assert_bytes(
        0x0045_be22,
        &[0x8b, 0x50, 0x50],
        "mov edx, [eax+0x50]: component list end",
    );
    assert_bytes(0x0045_be2f, &[0x8a, 0x48, 0x06], "mov cl, [eax+6]");

    // The win path and the no-win return.
    assert_bytes(0x0045_be40, &[0x6a, 0x01], "push 1");
    assert_bytes(
        0x0045_be47,
        &[0xe8, 0xc4, 0x7d, 0x00, 0x00],
        "call 0x463c10(1)",
    );
    assert_bytes(
        0x0045_be4c,
        &[0x68, 0x00, 0x00, 0x40, 0x40],
        "push 0x40400000: 3.0f",
    );
    assert_bytes(
        0x0045_be58,
        &[0xe8, 0xd3, 0x7d, 0x00, 0x00],
        "call 0x463c30(3.0f, 1)",
    );

    // The caller gates: the check runs only from CZMission::Update while the
    // mission state is 3, and that update is skipped entirely while the
    // player's +0x91d is set — the player-down pre-emption the binding's
    // OppositionFavour tie outcome records.
    assert_bytes(
        0x0046_a5b0,
        &[0xe8, 0xfb, 0x93, 0xff, 0xff],
        "call 0x4639b0: the mission-state predicate",
    );
    assert_bytes(
        0x0046_a5b7,
        &[0x74, 0x05],
        "je: not in state 3 skips the check",
    );
    assert_bytes(
        0x0046_a5b9,
        &[0xe8, 0x12, 0x14, 0xff, 0xff],
        "call 0x45b9d0",
    );
    assert_bytes(0x004a_0980, &[0x33, 0xdb], "xor ebx, ebx: bl is 0");
    assert_bytes(
        0x004a_09b1,
        &[0xa1, 0x98, 0xc2, 0x71, 0x00],
        "mov eax, [0x71c298]: the player vehicle",
    );
    assert_bytes(
        0x004a_09b6,
        &[0x38, 0x98, 0x1d, 0x09, 0x00, 0x00],
        "cmp [eax+0x91d], bl: the player-down byte",
    );
    assert_bytes(
        0x004a_09bc,
        &[0x75, 0x0c],
        "jne: player down skips the update",
    );
    assert_bytes(
        0x004a_09c3,
        &[0xe8, 0xc8, 0x9a, 0xfc, 0xff],
        "call 0x46a490: CZMission::Update",
    );
}

// -------------------------------------------------------- retail catalog ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Every retail preset's rules are the measured binding for its mission type,
/// and the option table offers only the designed conditions — the measured
/// ones name objects only the original spawn creates.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_rules_end_retail_each_preset_binds_its_mission_type_rules() {
    let catalog = retail_instant_action_catalog(&game_dir()).expect("the retail catalog assembles");
    assert!(!catalog.presets().is_empty());
    for preset in catalog.presets() {
        let mission_type = preset
            .original_mission_type()
            .expect("a measured mission type");
        let Declared::Declared(rules) = preset.parameters().rules() else {
            panic!(
                "{}: rules are the measured binding, not unmeasured",
                preset.id()
            );
        };
        assert_eq!(
            Some(*rules),
            measured_ia_victory_rules(mission_type),
            "{} binds the {mission_type} rules",
            preset.id()
        );
        assert_eq!(rules.respawns(), RespawnBudget::None);
        assert_eq!(rules.deadline_ticks(), None);
        assert_eq!(rules.tie_outcome(), TieOutcome::OppositionFavour);
    }
    assert_eq!(
        catalog.options().victory_conditions(),
        VictoryCondition::DESIGNED,
        "a custom scenario may never pick a condition that names the original's objects"
    );
}

// ------------------------------------------------------------- synthetic ---

fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_float(value: f32) -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A list node; the stored count is one more than the child count.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = 4_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

fn wrapped_text(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

fn spawn_list(count: usize) -> Vec<u8> {
    zrd_list(
        (0..count)
            .map(|_| {
                zrd_list(vec![
                    zrd_float(1.0),
                    zrd_float(2.0),
                    zrd_float(3.0),
                    zrd_float(4.0),
                ])
            })
            .collect(),
    )
}

/// An `ia.zrd` descriptor body offering `offered` mission types, in the same
/// shape `accept_f49_d_03_retail_catalog`'s synthetic descriptors use.
fn descriptor_bytes(offered: &[&str]) -> Vec<u8> {
    let mut children = vec![zrd_text("mission_type"), wrapped_text(offered[0])];
    children.push(zrd_text("player_plane"));
    children.push(wrapped_text("Fury"));
    children.push(zrd_text("num_wingmen"));
    children.push(zrd_list(vec![zrd_int(2)]));
    let mut spawn = Vec::new();
    for name in offered {
        spawn.push(zrd_text(name));
        spawn.push(spawn_list(2));
    }
    children.push(zrd_text("spawn_points"));
    children.push(zrd_list(spawn));
    children.push(zrd_text("group1"));
    children.push(zrd_list(vec![
        zrd_text("num_enemies"),
        zrd_list(vec![zrd_int(4)]),
        zrd_text("enemy_name"),
        wrapped_text("MSG_TEST_ENEMY"),
        zrd_text("enemy_plane"),
        wrapped_text("Kestrel"),
        zrd_text("enemy_skill"),
        wrapped_text("veteran"),
    ]));
    children.push(zrd_text("ace_name"));
    children.push(wrapped_text("MSG_TEST_ACE"));
    children.push(zrd_text("ace_plane"));
    children.push(wrapped_text("Peacemaker"));
    children.push(zrd_text("ace_skill"));
    children.push(wrapped_text("ace"));
    zrd_list(children)
}

fn directory(offered: &[&str]) -> cs_content::instant_action::IaDirectory {
    let root = decode_zrd(&descriptor_bytes(offered)).expect("the synthetic descriptor decodes");
    read_ia_descriptor(
        &root,
        "zbd/c1/ia1/zrdr.zbd",
        &"0".repeat(64),
        16,
        512,
        "c1",
        "ia1",
    )
    .expect("the synthetic descriptor reads")
}

/// An option table offering the real airframe rows (the descriptor's planes
/// must be offerable) over the fixture's other lists.
fn options_with_real_airframes() -> ScenarioOptions {
    let fixture = synthetic_scenario_options();
    let airframes: Vec<ContentId> = cs_app::mission_start::AIRFRAME_TABLE
        .iter()
        .map(|row| {
            ContentId::from_source(cs_types::content::ContentKind::Airframe, row.scene_root)
                .expect("a valid key")
        })
        .collect();
    ScenarioOptions::try_new(
        vec![WorldId::from_key("c1").expect("a world key")],
        fixture.environments().to_vec(),
        fixture.factions().to_vec(),
        airframes,
        fixture.loadouts().to_vec(),
        fixture.pilots().to_vec(),
        fixture.difficulty_tiers().to_vec(),
        fixture.victory_conditions().to_vec(),
        fixture.relations().to_vec(),
        fixture.max_players(),
    )
    .expect("the synthetic option table is valid")
}

fn inputs<'a>(options: &'a ScenarioOptions) -> DirectoryInputs<'a> {
    DirectoryInputs {
        install_sha256: cs_assets::install::sha256(b"synthetic installation"),
        container_path: "ZBD/C1/IA1/zrdr.zbd",
        options,
        environment: Err("the synthetic archive has no weather".to_owned()),
    }
}

/// The production preset assembly binds each offered mission type's measured
/// rules — the same `with_measured_rules` the retail catalog applies.
#[test]
fn accept_f49_d_03_fu_rules_end_a_descriptor_s_presets_carry_the_measured_rules() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let presets = directory_presets(&directory(&IA_MISSION_TYPE_VOCABULARY), &inputs)
        .expect("a descriptor offering all five types assembles");
    assert_eq!(presets.len(), 5);
    for preset in &presets {
        let mission_type = preset.original_mission_type().expect("a mission type");
        let Declared::Declared(rules) = preset.parameters().rules() else {
            panic!("{}: rules are the measured binding", preset.id());
        };
        assert_eq!(Some(*rules), measured_ia_victory_rules(mission_type));
    }
    // Removing the binding must fail the test: an unmeasured rules record is
    // what the presets carried before this measurement.
    assert!(
        presets.iter().all(|p| p.parameters().rules().is_declared()),
        "every offered type's rules are declared, not unmeasured"
    );
}

/// A catalog holding the fixture's dogfight preset with `rules` swapped in —
/// the production lowering gives a `LoweredScenario` whose condition is the
/// measured one.
fn catalog_with_rules(rules: VictoryRules) -> (InstantActionCatalog, ContentId) {
    let fixture = synthetic_instant_action_catalog();
    let preset = fixture.presets()[0].clone();
    let parameters = ScenarioParameters::new(
        preset.parameters().world().clone(),
        preset.parameters().environment().clone(),
        preset.parameters().roster().clone(),
        synthetic_difficulty(DifficultyTier::Standard),
        rules,
        preset.parameters().seed(),
    );
    let changed = InstantActionPreset::try_new(
        preset.id().clone(),
        preset.scenario().clone(),
        preset.title().clone(),
        parameters,
        preset.origin().clone(),
        preset.provenance().clone(),
    )
    .expect("the rebuilt preset is valid");
    let id = changed.id().clone();
    let catalog = InstantActionCatalog::try_new(
        vec![changed],
        fixture.options().clone(),
        fixture.provenance().clone(),
    )
    .expect("the rebuilt catalog is valid");
    (catalog, id)
}

fn measured_scenario(condition: VictoryCondition) -> (InstantActionCatalog, ContentId) {
    catalog_with_rules(
        VictoryRules::try_new(
            condition,
            RespawnBudget::None,
            None,
            TieOutcome::OppositionFavour,
        )
        .expect("measured rules are valid"),
    )
}

fn lowered(condition: VictoryCondition) -> cs_app::ui::instant_action::LoweredScenario {
    let (catalog, id) = measured_scenario(condition);
    lower_preset(&catalog, &id).expect("a declared preset lowers")
}

/// A snapshot naming the alive actors of `scenario`'s roster sides.
fn snapshot(scenario: &cs_app::ui::instant_action::LoweredScenario, tick: u64) -> ScenarioSnapshot {
    ScenarioSnapshot::new(
        tick,
        scenario.actors().iter().map(|actor| (actor.side, actor.slot)),
    )
}

/// The measured elimination predicates test the opposition only: a pending
/// (dormant) enemy satisfies `OppositionDown` but blocks `WavesCleared`, and
/// the player-down gate defeats either.
#[test]
fn accept_f49_d_03_fu_rules_end_the_measured_elimination_semantics() {
    let enemy_slot = |s: &cs_app::ui::instant_action::LoweredScenario| -> RosterSlot {
        s.actors_on(ScenarioSide::Enemy)
            .first()
            .expect("the fixture flies enemies")
            .slot
    };
    let player_alive = |s: &cs_app::ui::instant_action::LoweredScenario| -> ScenarioSnapshot {
        ScenarioSnapshot::new(
            0,
            s.actors_on(ScenarioSide::Player)
                .into_iter()
                .map(|actor| (actor.side, actor.slot)),
        )
    };

    // dogfight_ace / type 0: pending hostiles already satisfy the predicate.
    let ace = lowered(VictoryCondition::OppositionDown);
    let mut done = player_alive(&ace);
    assert_eq!(
        evaluate_outcome(&ace, &done).map(|o| o.result()),
        Some(ScenarioResult::Victory),
        "no live opponent wins the ace duel"
    );
    done = done.with_pending(ScenarioSide::Enemy, enemy_slot(&ace));
    assert_eq!(
        evaluate_outcome(&ace, &done).map(|o| o.result()),
        Some(ScenarioResult::Victory),
        "a dormant opponent satisfies the type-0 predicate (+0x945 unread)"
    );
    // Still running while a live opponent flies.
    assert!(
        evaluate_outcome(&ace, &snapshot(&ace, 0)).is_none(),
        "a live opponent keeps the scenario running"
    );

    // dogfight_squadron / type 1: a pending wave member blocks the win.
    let squadron = lowered(VictoryCondition::WavesCleared);
    assert_eq!(
        evaluate_outcome(&squadron, &player_alive(&squadron)).map(|o| o.result()),
        Some(ScenarioResult::Victory),
        "all waves cleared wins"
    );
    assert!(
        evaluate_outcome(
            &squadron,
            &player_alive(&squadron).with_pending(ScenarioSide::Enemy, enemy_slot(&squadron)),
        )
        .is_none(),
        "a dormant wave member is unsettled until its wave is woken and downed"
    );

    // The player-down pre-emption defeats even a completed opposition — the
    // original's caller cannot run the check on such a tick, and the declared
    // OppositionFavour tie resolves the mutual wipe against the player.
    let nobody = ScenarioSnapshot::new(0, []);
    for condition in [
        VictoryCondition::OppositionDown,
        VictoryCondition::WavesCleared,
    ] {
        let scenario = lowered(condition);
        assert_eq!(
            evaluate_outcome(&scenario, &nobody).map(|o| o.result()),
            Some(ScenarioResult::Defeat),
            "{condition}: player down pre-empts the win"
        );
        assert_eq!(scenario.respawns(), RespawnBudget::None);
        assert_eq!(scenario.deadline_ticks(), None);
        assert_eq!(scenario.tie_outcome(), TieOutcome::OppositionFavour);
    }
}

/// The object-bound conditions end on `objective_done` — the snapshot's
/// stand-in for the bound zeppelin, target wrapper and gate list — and are
/// indifferent to the actor count, as the measured tails are.
#[test]
fn accept_f49_d_03_fu_rules_end_the_object_bound_conditions_end_on_the_object() {
    for condition in [
        VictoryCondition::ZeppelinDown,
        VictoryCondition::TargetDown,
        VictoryCondition::AllGatesCleared,
    ] {
        let scenario = lowered(condition);
        // Everyone alive, objective not done: still running.
        assert!(
            evaluate_outcome(&scenario, &snapshot(&scenario, 0)).is_none(),
            "{condition}: an unconsumed objective keeps running"
        );
        // The objective ends it while the opposition still flies — the
        // measured tails do not consult the actor list.
        assert_eq!(
            evaluate_outcome(&scenario, &snapshot(&scenario, 0).with_objective_done())
                .map(|o| o.result()),
            Some(ScenarioResult::Victory),
            "{condition}: the bound object's end is the win"
        );
        // Player down pre-empts it too.
        assert_eq!(
            evaluate_outcome(
                &scenario,
                &ScenarioSnapshot::new(0, []).with_objective_done()
            )
            .map(|o| o.result()),
            Some(ScenarioResult::Defeat),
            "{condition}: player down pre-empts even a completed objective"
        );
    }
}

/// The measured conditions may be bound per mission type but never offered:
/// the option-table validator refuses them so a custom scenario cannot pick a
/// predicate over objects only the original spawn creates.
#[test]
fn accept_f49_d_03_fu_rules_end_measured_conditions_are_never_offered() {
    assert_eq!(VictoryCondition::DESIGNED.len(), 3);
    for condition in [
        VictoryCondition::OppositionDown,
        VictoryCondition::WavesCleared,
        VictoryCondition::ZeppelinDown,
        VictoryCondition::TargetDown,
        VictoryCondition::AllGatesCleared,
    ] {
        assert!(!VictoryCondition::DESIGNED.contains(&condition));
        let fixture = synthetic_scenario_options();
        assert_eq!(
            ScenarioOptions::try_new(
                fixture.worlds().to_vec(),
                fixture.environments().to_vec(),
                fixture.factions().to_vec(),
                fixture.airframes().to_vec(),
                fixture.loadouts().to_vec(),
                fixture.pilots().to_vec(),
                fixture.difficulty_tiers().to_vec(),
                vec![condition],
                fixture.relations().to_vec(),
                fixture.max_players(),
            ),
            Err(ScenarioSchemaError::UnknownVictoryCondition { condition }),
            "{condition} is bound per mission_type, never offered"
        );
    }
}

/// The finding the claim names exists and states the measurement.
#[test]
fn accept_f49_d_03_fu_rules_end_the_finding_records_the_measurement() {
    assert_eq!(IA_RULES_END_CLAIM, "f49.d.03.ia-rules-end");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let finding = fs::read_to_string(root.join(FINDING)).expect("the finding is committed");
    for needle in [
        IA_RULES_END_CLAIM,
        "0x45b9d0",
        "0x718cd8",
        "0x458c20",
        "+0x91d",
        "+0x945",
        "+0x388",
        "unmeasured",
        "dogfight_ace",
        "dogfight_squadron",
        "zeppelin_run",
        "ground_target",
        "stunt_flying",
    ] {
        assert!(finding.contains(needle), "the finding mentions {needle}");
    }
}

/// The condition the engine's type index dispatches to — the asserted mapping
/// between the parser's produced index and the production binding.
fn expected_condition(index: u32) -> VictoryCondition {
    match index {
        0 => VictoryCondition::OppositionDown,
        1 => VictoryCondition::WavesCleared,
        2 => VictoryCondition::ZeppelinDown,
        3 => VictoryCondition::TargetDown,
        4 => VictoryCondition::AllGatesCleared,
        _ => unreachable!("the parser's range is 0..=4"),
    }
}
