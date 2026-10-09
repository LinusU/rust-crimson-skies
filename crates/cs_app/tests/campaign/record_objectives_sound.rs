//! Acceptance stage RECORD-OBJECTIVES-SOUND: the original's parser spells two
//! record-level sound keys M02-B-FU2 (#801) left unadmitted —
//! `OBJECTIVES_WON_SOUND` and `OBJECTIVES_LOST_SOUND` — and this stage
//! measures them by the same method and admits them to
//! `CONTROL_RECORD_SOUND_KEY_VOCABULARY` (Rally #808). The parser blocks are
//! the same shape as the five M02 keys' — push the key's string, store a zero
//! default, look the child up by name (`0x57a090`), resolve that name to a
//! handle (`0x596120`), store the handle in the mission object — and the
//! consumer is the end-of-tick outcome block in `CZMission::Update`: the lost
//! flag is tested first, the won flag only when the lost one is clear, each
//! branch runs the mission-end routine `0x463c30` and then plays its handle
//! through the sound manager global `0x71b438` when the handle is non-null.
//!
//! The admission is about vocabulary completeness, not shipped content: no
//! mission-scoped reader archive in the owner's installation spells either
//! key — the retail member below is the complete `cs_app::mission_control`
//! census re-walked member by member, so the claim rests on production
//! discovery rather than on the strings' absence from the executable. A
//! mission that spells one is measured the day it ships; until then nothing
//! changes for any retail record, and nothing here plays a sound or is
//! `verified_original` (AGENTS.md rules 4 and 8): a measured disposition is a
//! statement about the original's code, not a licence. The measurements live
//! in `docs/findings/2026-10-09-record-objectives-sound-keys.md`.
//!
//! The three members, and where each one fails if the implementation is
//! removed:
//!
//! * [`accept_record_objectives_sound_the_two_keys_are_measured_with_their_outcome_consumer`]
//!   (synthetic, CI) holds the vocabulary, the enum surface and the
//!   disposition table to each other: a vocabulary that lost either key, a
//!   `from_key` that stopped classifying it or a disposition that lost its
//!   consumer fails here.
//! * [`accept_record_objectives_sound_the_image_gates_and_consumes_both_keys_at_end_of_tick`]
//!   (engine image) reads `$CS_ENGINE_IMAGE` at the addresses the production
//!   table records and re-derives the selector — the lost-flag-first branch
//!   order, the null-handle skips and the shared play call — from the
//!   instruction bytes, so an address, a field offset or a branch direction
//!   that drifts from the original fails here.
//! * [`accept_record_objectives_sound_no_retail_control_member_spells_either_key`]
//!   (retail) runs the production census over the whole installation and
//!   re-walks every measured row's control member independently, so a mission
//!   that started spelling either key — or a vocabulary that stopped counting
//!   it — fails here.
//!
//! The retail member is `#[ignore = "requires CS_GAME_DIR"]` and the image
//! member `#[ignore = "requires CS_ENGINE_IMAGE"]`, so CI (which has neither)
//! skips both and the implementing and reviewing agents run them with
//! `--include-ignored`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::{ControlProgram, survey_mission_control_programs};
use cs_assets::install::sha256;
use cs_content::coordinates::ORIGINAL_IMAGE_SHA256;
use cs_content::mission_control::{
    CONTROL_RECORD_KEY_VOCABULARY, CONTROL_RECORD_SOUND_KEY_VOCABULARY, ControlRecordField,
    ControlRecordSound, MeasuredRecordSound, RecordSoundConsumer, RecordSoundDisposition,
    record_sound_disposition,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

/// The image's base address, and with it the file-offset rule this task's
/// measurements are written in: `VA = file offset + 0x400000`.
///
/// Measured on `$CS_ENGINE_IMAGE` (PE32, image base `0x400000`) by walking its
/// section table: `.text` is `VA 0x401000` at file `0x1000` and `.data` is
/// `VA 0x219000` at file `0x219000`, so for the two sections this stage reads
/// — the code in `.text`, the key strings in `.data` — the RVA equals the file
/// offset. Every address below is a virtual address, converted by this
/// constant alone.
const IMAGE_BASE: u32 = 0x40_0000;

/// The two keys this task admits, in the original parser's order (the parse
/// chain itself spells won before lost: the won block's not-found jump lands
/// on the lost block's head, and the lost block's lands on
/// `MISSION_WON_SOUND`'s).
const ADMITTED: [ControlRecordSound; 2] = [
    ControlRecordSound::ObjectivesWon,
    ControlRecordSound::ObjectivesLost,
];

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: RECORD-OBJECTIVES-SOUND needs the retail capability; run \
             this suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only \
             installation"
        )
    }))
}

/// The owner's decrypted executable, as the environment declares it.
fn engine_image() -> Vec<u8> {
    let path = PathBuf::from(std::env::var("CS_ENGINE_IMAGE").unwrap_or_else(|_| {
        panic!(
            "CS_ENGINE_IMAGE is not set: RECORD-OBJECTIVES-SOUND measures the original's code in \
             the owner-supplied decrypted image; run this suite with `--include-ignored` and \
             CS_ENGINE_IMAGE pointing at it (AGENTS.md, Environment)"
        )
    }));
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the engine image {} reads: {error}", path.display()))
}

/// The image bytes as `u32` little-endian at `offset`, checked against the
/// file so a bad address is a named failure rather than a slice panic.
fn u32_at(image: &[u8], offset: usize) -> u32 {
    let bytes: [u8; 4] = image
        .get(offset..offset + 4)
        .unwrap_or_else(|| panic!("file offset 0x{offset:x} lies inside the image"))
        .try_into()
        .expect("four bytes");
    u32::from_le_bytes(bytes)
}

/// The byte at `offset`, checked against the file.
fn byte_at(image: &[u8], offset: usize) -> u8 {
    *image
        .get(offset)
        .unwrap_or_else(|| panic!("file offset 0x{offset:x} lies inside the image"))
}

/// File offset of a virtual address (see [`IMAGE_BASE`]).
fn file_offset(va: u32) -> usize {
    (va - IMAGE_BASE) as usize
}

/// The virtual address a `call` at virtual address `va` (opcode `e8`)
/// transfers to: the operand is `rel32` relative to the end of the
/// instruction.
fn call_target(image: &[u8], va: u32) -> u32 {
    let offset = file_offset(va);
    assert_eq!(byte_at(image, offset), 0xe8, "a near call at VA 0x{va:x}");
    let relative = i32::from_le_bytes(
        image[offset + 1..offset + 5]
            .try_into()
            .expect("a rel32 operand"),
    );
    (va + 5).wrapping_add_signed(relative)
}

/// The virtual address a two-byte short branch at virtual address `va`
/// transfers to: a conditional `7x rel8` or the unconditional `eb rel8`, with
/// the operand relative to the next instruction.
fn short_jump_target(image: &[u8], va: u32) -> u32 {
    let offset = file_offset(va);
    let opcode = byte_at(image, offset);
    assert!(
        opcode & 0xf0 == 0x70 || opcode == 0xeb,
        "a short branch at VA 0x{va:x}, got opcode 0x{opcode:02x}"
    );
    let relative = i8::from_le_bytes([byte_at(image, offset + 1)]);
    let target = i32::try_from(va).expect("the VA fits in i32") + 2 + i32::from(relative);
    u32::try_from(target).expect("a forward target")
}

/// The NUL-terminated bytes the image holds at virtual address `va`, exactly
/// as `expected` spells them, checked against the file's own length.
fn string_at(image: &[u8], va: u32, expected: &str) {
    let start = file_offset(va);
    let found = image
        .get(start..start + expected.len() + 1)
        .unwrap_or_else(|| panic!("the string at VA 0x{va:x} lies inside the image"));
    assert_eq!(
        found,
        [expected.as_bytes(), &[0]].concat(),
        "the string at VA 0x{va:x} is {expected:?} plus its terminator"
    );
}

/// One admitted key's measured disposition, or a named failure when the
/// implementation under test is gone.
fn measured(key: ControlRecordSound) -> MeasuredRecordSound {
    let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key.key())
    else {
        panic!("{} is in the vocabulary and measured", key.key());
    };
    measured
}

/// **The two `OBJECTIVES_*_SOUND` keys are admitted and measured: the
/// vocabulary carries them in the parser's order, `from_key` classifies them
/// as sounds, and each disposition names the end-of-tick outcome consumer.**
/// Every assertion reads production surface only, so deleting the variants,
/// the dispositions or the vocabulary entries fails here without any original
/// data.
#[test]
fn accept_record_objectives_sound_the_two_keys_are_measured_with_their_outcome_consumer() {
    // The vocabulary is seven keys — M02-B-FU2's five plus this task's two —
    // and the two sit where the original's parse chain puts them: between
    // `TERTIARY_COMPLETE_SOUND` and `MISSION_WON_SOUND`.
    assert_eq!(
        CONTROL_RECORD_SOUND_KEY_VOCABULARY,
        [
            "PRIMARY_COMPLETE_SOUND",
            "SECONDARY_COMPLETE_SOUND",
            "TERTIARY_COMPLETE_SOUND",
            "OBJECTIVES_WON_SOUND",
            "OBJECTIVES_LOST_SOUND",
            "MISSION_WON_SOUND",
            "MISSION_LOST_SOUND",
        ],
        "the sound vocabulary is the parser's seven keys in parse order"
    );
    for key in CONTROL_RECORD_SOUND_KEY_VOCABULARY {
        assert!(
            !CONTROL_RECORD_KEY_VOCABULARY.contains(&key),
            "{key} belongs to the sound vocabulary only"
        );
    }

    // The enum admits them and `from_key`/`key` round-trip. `ALL` is the
    // vocabulary in the same parse order — the two admitted variants sit
    // where the parse chain puts them.
    assert_eq!(
        ControlRecordSound::ALL.map(ControlRecordSound::key),
        CONTROL_RECORD_SOUND_KEY_VOCABULARY,
        "ALL is the vocabulary, in the same parse order"
    );
    for (sound, spelling, won) in [
        (
            ControlRecordSound::ObjectivesWon,
            "OBJECTIVES_WON_SOUND",
            true,
        ),
        (
            ControlRecordSound::ObjectivesLost,
            "OBJECTIVES_LOST_SOUND",
            false,
        ),
    ] {
        assert_eq!(sound.key(), spelling);
        assert_eq!(ControlRecordSound::from_key(spelling), Some(sound));
        assert_eq!(
            ControlRecordField::from_key(spelling),
            Some(ControlRecordField::Sound(sound)),
            "{spelling} classifies as a record-level sound field"
        );

        let measured = measured(sound);
        assert_eq!(
            measured.consumer,
            RecordSoundConsumer::ObjectivesOutcome { won },
            "{spelling} is consumed by the end-of-tick outcome block"
        );
        assert_eq!(measured.consumer.code(), "objectives_outcome");
        assert_eq!(
            measured.consumer.class(),
            None,
            "{spelling} is not gated on an objective's presentation class"
        );
        assert!(
            !measured.summary.is_empty()
                && !measured.evidence.is_empty()
                && !measured.unknowns.is_empty(),
            "{spelling} names its effect, its evidence and the unknowns it leaves"
        );
        for finding in [
            "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics",
            "2026-10-06-m01-lc-directive-d-sound-help-timer-directives",
            "2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering",
            "2026-10-09-record-objectives-sound-keys",
        ] {
            assert!(
                measured.evidence.contains(&finding),
                "{spelling} cites {finding}"
            );
        }
        assert!(
            measured.field_offset != 0 && measured.parse_site != 0 && measured.consumer_site != 0,
            "{spelling} carries the addresses it was read at"
        );
    }

    // The two fields are distinct and sit beside the sibling handles: the
    // parser's write order is tertiary +0xc6c, won +0xc70, lost +0xc74,
    // mission-won +0xc78.
    assert_eq!(
        measured(ControlRecordSound::ObjectivesWon).field_offset,
        0xc70
    );
    assert_eq!(
        measured(ControlRecordSound::ObjectivesLost).field_offset,
        0xc74
    );
    assert_eq!(measured(ControlRecordSound::MissionWon).field_offset, 0xc78);
}

/// **Both keys parse where production says they parse, and the end-of-tick
/// outcome block consumes them in the order production records — read out of
/// the owner's decrypted image, not out of the table.** The instruction bytes
/// re-derive the whole selector: the lost flag is tested first, its clear
/// branch lands on the won test, each branch runs the mission-end routine
/// before loading its handle, a null handle skips the shared play call, and
/// `Update`'s head returns early on the ended flag so the block runs at most
/// once. A `parse_site`, `field_offset` or `consumer_site` in
/// `cs_content::mission_control` that drifts from the original fails here.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_record_objectives_sound_the_image_gates_and_consumes_both_keys_at_end_of_tick() {
    let image = engine_image();
    assert_eq!(
        sha256(&image).to_hex(),
        ORIGINAL_IMAGE_SHA256,
        "the image is the one every measurement here was read from"
    );

    // Each key's parse block: `push <key string>`, `push ebp`, `mov
    // [ebx+field], edi` (the zero default), the by-name child lookup, the
    // name-to-handle resolution and `mov [ebx+field], eax` (the store).
    for sound in ADMITTED {
        let measured = measured(sound);
        let parse = file_offset(measured.parse_site);
        assert_eq!(
            byte_at(&image, parse),
            0x68,
            "{}: the parse site is a `push imm32` of the key's string",
            sound.key()
        );
        let string_va = u32_at(&image, parse + 1);
        string_at(&image, string_va, sound.key());
        assert_eq!(
            [
                byte_at(&image, parse + 5),
                byte_at(&image, parse + 6),
                byte_at(&image, parse + 7)
            ],
            [0x55, 0x89, 0xbb],
            "{}: the parser pushes the record and writes the zero default",
            sound.key()
        );
        assert_eq!(
            u32_at(&image, parse + 8),
            measured.field_offset,
            "{}: the zero default lands in the recorded field",
            sound.key()
        );
        assert_eq!(
            call_target(&image, measured.parse_site + 0xc),
            0x57_a090,
            "{}: the parse looks the key up in the record by name",
            sound.key()
        );
        assert_eq!(
            call_target(&image, measured.parse_site + 0x1f),
            0x59_6120,
            "{}: the parse resolves the name through the name-to-handle lookup",
            sound.key()
        );
        assert_eq!(
            [byte_at(&image, parse + 0x27), byte_at(&image, parse + 0x28)],
            [0x89, 0x83],
            "{}: the resolved handle is stored into the mission object",
            sound.key()
        );
        assert_eq!(
            u32_at(&image, parse + 0x29),
            measured.field_offset,
            "{}: the handle lands in the recorded field",
            sound.key()
        );
    }

    // The parse chain's own order: the won block's not-found jump lands on
    // the lost block's head, and the lost block's lands on the
    // `MISSION_WON_SOUND` block — so the vocabulary's parse order is the
    // original's, not this task's choice.
    let won = measured(ControlRecordSound::ObjectivesWon);
    let lost = measured(ControlRecordSound::ObjectivesLost);
    assert_eq!(
        short_jump_target(&image, won.parse_site + 0x16),
        lost.parse_site,
        "the won block's child-not-found jump lands on the lost block's head"
    );
    assert_eq!(
        short_jump_target(&image, lost.parse_site + 0x16),
        measured(ControlRecordSound::MissionWon).parse_site,
        "the lost block's child-not-found jump lands on the mission-won block's head"
    );

    // The three flag accessors the block's branches are built from: bare
    // `mov eax, [ecx+field]; ret` stubs, so the fields the branches test are
    // +0xc5c (lost), +0xc58 (won) and +0xc54 (ended).
    for (accessor, field) in [
        (0x46_3bf0_u32, 0xc5c_u32),
        (0x46_3be0, 0xc58),
        (0x46_3c00, 0xc54),
    ] {
        let at = file_offset(accessor);
        assert_eq!(
            &image[at..at + 7],
            &[
                0x8b,
                0x81,
                field as u8,
                (field >> 8) as u8,
                0x00,
                0x00,
                0xc3
            ],
            "accessor 0x{accessor:x} is `mov eax, [ecx+0x{field:x}]; ret`"
        );
    }

    // The end-of-tick outcome block in `CZMission::Update` (0x46af76 on), and
    // the once-only gate above it: `Update` tests the ended flag near its
    // head and jumps past its body when it is set, so the block's mission-end
    // call cannot run a second tick.
    assert_eq!(
        call_target(&image, 0x46_a9b7),
        0x46_3c00,
        "Update's head tests the ended flag"
    );
    assert_eq!(
        byte_at(&image, file_offset(0x46_a9be)),
        0x75,
        "an ended mission skips the rest of Update"
    );

    // Lost first: test the lost flag, and only when it is clear fall to the
    // won branch.
    assert_eq!(
        call_target(&image, 0x46_af7a),
        0x46_3bf0,
        "the outcome block tests the lost flag first"
    );
    let won_branch = short_jump_target(&image, 0x46_af81);
    assert_eq!(
        won_branch, 0x46_afab,
        "a clear lost flag jumps to the won test, not past the block"
    );
    assert_eq!(
        call_target(&image, won_branch + 2),
        0x46_3be0,
        "the won branch tests the won flag"
    );

    // Each branch runs the mission-end routine, then loads its handle and
    // skips the shared play call when the handle is null.
    let end_mission = 0x46_3c30;
    assert_eq!(
        call_target(&image, 0x46_af9a),
        end_mission,
        "the lost branch ends the mission"
    );
    assert_eq!(
        call_target(&image, 0x46_afcd),
        end_mission,
        "the won branch ends the mission"
    );
    let play_block = 0x46_afdc;
    let past_play = 0x46_aff0;
    for (sound, load_va, skip_va) in [
        (
            ControlRecordSound::ObjectivesLost,
            0x46_af9f_u32,
            0x46_afa7_u32,
        ),
        (ControlRecordSound::ObjectivesWon, 0x46_afd2, 0x46_afda),
    ] {
        let measured = measured(sound);
        assert_eq!(
            measured.consumer_site,
            load_va,
            "{}'s consumer site is the field load in the outcome block",
            sound.key()
        );
        let load = file_offset(load_va);
        assert_eq!(
            [byte_at(&image, load), byte_at(&image, load + 1)],
            [0x8b, 0x83],
            "{}: the outcome block loads `mov eax, [ebx+field]` — Update's `this` is ebx",
            sound.key()
        );
        assert_eq!(
            u32_at(&image, load + 2),
            measured.field_offset,
            "{}: the consumer reads the field the parser wrote",
            sound.key()
        );
        assert_eq!(
            short_jump_target(&image, skip_va),
            past_play,
            "{}: a null handle skips the shared play call",
            sound.key()
        );
    }
    // The lost branch's non-null arm jumps forward to join the won branch's
    // fall-through at the play block; the won flag's clear arm skips both.
    assert_eq!(
        short_jump_target(&image, 0x46_afa9),
        play_block,
        "a non-null lost handle joins the shared play block"
    );
    assert_eq!(
        short_jump_target(&image, 0x46_afb4),
        0x46_afff,
        "a clear won flag leaves the outcome block entirely"
    );

    // The shared play call: `push 0; push 1; push 1.0f; push eax` — the
    // handle — then the sound manager global and the measured call.
    let play = file_offset(play_block);
    assert_eq!(
        &image[play..play + 10],
        &[0x6a, 0x00, 0x6a, 0x01, 0x68, 0x00, 0x00, 0x80, 0x3f, 0x50],
        "the play block passes the handle with the measured argument shape"
    );
    assert_eq!(
        byte_at(&image, file_offset(0x46_afe6)),
        0xb9,
        "the play block loads the sound manager global into ecx"
    );
    assert_eq!(
        u32_at(&image, file_offset(0x46_afe7)),
        0x71_b438,
        "the sound manager global is the measured one"
    );
    assert_eq!(
        call_target(&image, 0x46_afeb),
        0x46_caf0,
        "the shared play call is the measured sound-play routine"
    );
}

/// **No retail control member spells either admitted key — measured over the
/// whole installation by the production census, then re-derived member by
/// member.** The census is the check of record: every measured row's
/// `record_sounds()` must carry neither admitted variant. The independent
/// walk decodes each measured row's control member a second time through
/// `discover_container` / `decode_zrd` / `zrd_flat_fields` and partitions its
/// record-level keys by the vocabulary constants, so a `measure_control_record`
/// that dropped or absorbed a spelling cannot pass. The pin is non-vacuous:
/// the corpus does spell other sound keys (M02's five), so a walk that found
/// no sound keys at all would fail here too.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_record_objectives_sound_no_retail_control_member_spells_either_key() {
    let census = survey_mission_control_programs(&game_dir())
        .expect("the installation measures a control census");
    assert!(
        census.measured_len() >= 30,
        "the census measured the mission corpus (got {} measured rows)",
        census.measured_len()
    );

    let mut sound_spellers = 0;
    let mut offenders: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for row in census.measured_rows() {
        let Some(record) = row.record() else {
            continue;
        };
        if !record.record_sounds().is_empty() {
            sound_spellers += 1;
        }
        for (sound, _sites) in record.record_sounds() {
            if ADMITTED.contains(&sound) {
                offenders
                    .entry(row.mission.clone())
                    .or_default()
                    .push(sound.key());
            }
        }
        for key in record.unclassified_record_keys() {
            assert!(
                !ADMITTED.iter().any(|sound| sound.key() == key.as_str()),
                "{}'s record spells {key}, which is classified now — the walk must see it as a \
                 sound key, not as unclassified",
                row.mission
            );
        }
    }
    assert!(
        offenders.is_empty(),
        "no retail control member spells an OBJECTIVES_*_SOUND key: {offenders:?}"
    );
    assert!(
        sound_spellers >= 1,
        "the corpus does spell record sound keys (M02's five), so the zero above is measured, \
         not a walk that found nothing"
    );

    // Independent re-walk: decode each measured row's control member from its
    // archive a second time and partition its record-level keys by the
    // vocabulary constants rather than by `measure_control_record`.
    for row in census.measured_rows() {
        let ControlProgram::Measured { member, .. } = &row.program else {
            continue;
        };
        let bytes = std::fs::read(game_dir().join(&row.container))
            .unwrap_or_else(|error| panic!("{} reads: {error}", row.container));
        let relative =
            RelativePath::new(&row.container.to_lowercase()).expect("the archive path is relative");
        let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
        assert!(
            discovery.findings().is_empty(),
            "{} locates without findings: {:?}",
            row.container,
            discovery.findings()
        );
        let mut decoded = None;
        for program in discovery.programs() {
            if program.locator().member() == Some(member.as_str()) {
                decoded = Some(
                    decode_zrd(program.bytes())
                        .unwrap_or_else(|error| panic!("{member} decodes: {error}")),
                );
            }
        }
        let document = decoded.unwrap_or_else(|| panic!("{member} is in {}", row.container));
        let spelled: Vec<&str> = zrd_flat_fields(objective_record(&document))
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| objective_block_number(key).is_none())
            .collect();
        for key in spelled {
            assert!(
                !ADMITTED.iter().any(|sound| sound.key() == key),
                "{}'s control member {member} spells {key} at record level — the census result \
                 above must carry it as a sound row",
                row.mission
            );
        }
    }
}
