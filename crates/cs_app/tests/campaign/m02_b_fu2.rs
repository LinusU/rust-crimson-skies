//! Acceptance stage M02-B-FU2: the five record-level sound keys M02's control
//! record spells outside its numbered blocks — `PRIMARY_COMPLETE_SOUND`,
//! `SECONDARY_COMPLETE_SOUND`, `TERTIARY_COMPLETE_SOUND`, `MISSION_WON_SOUND`
//! and `MISSION_LOST_SOUND` — are measured instead of merely counted
//! (`missions/M02.md`, work order `M02-B`, follow-up Rally #801).
//!
//! M02-B counted those five keys and filed them as unknown: they sit outside
//! `CONTROL_RECORD_KEY_VOCABULARY`, so `measure_control_record` reported them
//! unclassified and never interpreted them. This stage measures what the
//! original does with each one in the M01-LC finding family's method — static
//! reading of the owner's decrypted executable at `$CS_ENGINE_IMAGE`, with
//! residual unknowns and no runtime claim — and admits them to
//! `cs_content::mission_control` as their own measured vocabulary
//! (`CONTROL_RECORD_SOUND_KEY_VOCABULARY` +
//! `record_sound_disposition`), each carrying its consumer. Nothing here plays
//! a sound, and nothing is `verified_original` (AGENTS.md rule 8): a measured
//! disposition is a statement about the original, not a licence. The
//! measurements themselves live in
//! `docs/findings/2026-10-09-m02-b-fu2-record-sound-keys.md`.
//!
//! The three members, and where each one fails if the implementation is
//! removed:
//!
//! * [`accept_m02_b_fu2_m02s_five_record_sound_keys_are_measured_with_their_consumers`]
//!   (retail) re-walks M02's decoded control member independently of
//!   `measure_control_record`, so a record walk that dropped or absorbed a
//!   sound key, a disposition that lost its consumer or an empty
//!   `unclassified_record_keys` that was only ever vacuous all fail here.
//! * [`accept_m02_b_fu2_the_image_parses_and_consumes_each_sound_key_where_production_says`]
//!   (engine image) reads `$CS_ENGINE_IMAGE` at the addresses the production
//!   table records and re-derives both selectors — the objective-class chain
//!   and the mission won flag — from the instruction bytes, so an address, a
//!   field offset or a class that drifts from the original fails here.
//! * [`accept_m02_b_fu2_the_sound_vocabulary_is_entirely_measured_and_answers_for_nothing_else`]
//!   (synthetic, CI) holds the vocabulary and the disposition table to each
//!   other: a vocabulary key with no measurement, or a spelling the table
//!   answers for outside the vocabulary, fails here.
//!
//! The retail member is `#[ignore = "requires CS_GAME_DIR"]` and the image
//! member `#[ignore = "requires CS_ENGINE_IMAGE"]`, so CI (which has neither)
//! skips both and the implementing and reviewing agents run them with
//! `--include-ignored`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_assets::install::sha256;
use cs_content::coordinates::ORIGINAL_IMAGE_SHA256;
use cs_content::mission_control::{
    CONTROL_RECORD_KEY_VOCABULARY, CONTROL_RECORD_SOUND_KEY_VOCABULARY, ControlRecordSound,
    RecordSoundConsumer, RecordSoundDisposition, record_sound_disposition,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{objective_record, zrd_flat_fields};

use crate::m02_b::{control_binding, control_document};

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

/// The owner's decrypted executable, as the environment declares it.
fn engine_image() -> Vec<u8> {
    let path = PathBuf::from(std::env::var("CS_ENGINE_IMAGE").unwrap_or_else(|_| {
        panic!(
            "CS_ENGINE_IMAGE is not set: M02-B-FU2 measures the original's code in the \
             owner-supplied decrypted image; run this suite with `--include-ignored` and \
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

/// The virtual address a `call` at file `offset` (opcode `e8`) transfers to:
/// the operand is `rel32` relative to the end of the instruction.
fn rel32_target(image: &[u8], offset: usize) -> u32 {
    assert_eq!(
        byte_at(image, offset),
        0xe8,
        "a near call at file 0x{offset:x}"
    );
    let relative = i32::from_le_bytes(
        image[offset + 1..offset + 5]
            .try_into()
            .expect("a rel32 operand"),
    );
    // offset is a file offset; the instruction's VA is offset + IMAGE_BASE,
    // and the operand is relative to the *next* instruction (+5).
    (offset as u32) + IMAGE_BASE + 5u32.wrapping_add_signed(relative)
}

/// The virtual address a two-byte short jump or conditional at virtual
/// address `va` transfers to: the operand is `rel8` relative to the next
/// instruction.
fn rel8_target(image: &[u8], va: u32) -> u32 {
    let offset = file_offset(va);
    assert_eq!(
        byte_at(image, offset) & 0xf0,
        0x70,
        "a short conditional jump at VA 0x{va:x}"
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

/// **M02's five record-level sound keys are classified and measured: each one
/// is counted by its own vocabulary, each disposition names the consumer that
/// reads the handle, and nothing M02 spells outside its blocks is left
/// unclassified.** The record-level partition is re-derived here by walking
/// the decoded member a second time through `zrd_flat_fields`, classifying by
/// the two vocabulary constants rather than by the walk under test, so this
/// cannot pass on a `measure_control_record` that dropped or absorbed a key.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_fu2_m02s_five_record_sound_keys_are_measured_with_their_consumers() {
    let binding = control_binding();
    let record = &binding.record;
    let (document, _) = control_document();

    // The two vocabularies are disjoint, so a key can never be counted twice.
    for key in CONTROL_RECORD_SOUND_KEY_VOCABULARY {
        assert!(
            !CONTROL_RECORD_KEY_VOCABULARY.contains(&key),
            "{key} belongs to the sound vocabulary only"
        );
    }

    // Independent walk: every record-level key is in exactly one vocabulary.
    let mut fields: Vec<&str> = Vec::new();
    let mut sounds: Vec<&str> = Vec::new();
    let mut unclassified: Vec<&str> = Vec::new();
    for (key, _value) in zrd_flat_fields(objective_record(&document)) {
        if objective_block_number(key).is_some() {
            continue;
        }
        if CONTROL_RECORD_KEY_VOCABULARY.contains(&key) {
            fields.push(key);
        } else if CONTROL_RECORD_SOUND_KEY_VOCABULARY.contains(&key) {
            sounds.push(key);
        } else {
            unclassified.push(key);
        }
    }
    fields.sort_unstable();
    sounds.sort_unstable();
    assert_eq!(
        fields,
        {
            let mut expected = CONTROL_RECORD_KEY_VOCABULARY;
            expected.sort_unstable();
            expected
        },
        "the record-level field keys are exactly the shape-measured vocabulary"
    );
    // M02 spells five of the seven vocabulary keys: the two
    // `OBJECTIVES_*_SOUND` keys RECORD-OBJECTIVES-SOUND (#808) admitted are
    // spelled by no retail record, so the vocabulary is a superset of what a
    // single mission can carry and the check is which five M02 spells.
    let m02_sounds = [
        ControlRecordSound::PrimaryComplete,
        ControlRecordSound::SecondaryComplete,
        ControlRecordSound::TertiarComplete,
        ControlRecordSound::MissionWon,
        ControlRecordSound::MissionLost,
    ];
    assert_eq!(
        sounds,
        {
            let mut expected: Vec<&str> = m02_sounds.iter().map(|sound| sound.key()).collect();
            expected.sort_unstable();
            expected
        },
        "the record-level sound keys M02 spells are exactly its five measured keys"
    );
    assert!(
        unclassified.is_empty(),
        "no M02 record-level key is left outside both vocabularies: {unclassified:?}"
    );

    // The production measurement reports the same partition.
    assert_eq!(
        record.record_fields().len(),
        CONTROL_RECORD_KEY_VOCABULARY.len() + m02_sounds.len(),
        "the measured record fields are the shape vocabulary and M02's five sound keys"
    );
    let sound_rows = record.record_sounds();
    let spelled: Vec<(&str, u32)> = sound_rows
        .iter()
        .map(|(key, sites)| (key.key(), *sites))
        .collect();
    assert_eq!(
        spelled,
        m02_sounds
            .iter()
            .map(|key| (key.key(), 1))
            .collect::<Vec<_>>(),
        "each sound key is counted once, in the vocabulary's parse order"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M02 leaves no record-level key unclassified: {:?}",
        record.unclassified_record_keys()
    );

    // Every key is measured, and each disposition names its consumer.
    let expected_consumers = [
        (
            ControlRecordSound::PrimaryComplete,
            RecordSoundConsumer::ObjectiveCompletion { class: 1 },
        ),
        (
            ControlRecordSound::SecondaryComplete,
            RecordSoundConsumer::ObjectiveCompletion { class: 2 },
        ),
        (
            ControlRecordSound::TertiaryComplete,
            RecordSoundConsumer::ObjectiveCompletion { class: 3 },
        ),
        (
            ControlRecordSound::MissionWon,
            RecordSoundConsumer::MissionEnd,
        ),
        (
            ControlRecordSound::MissionLost,
            RecordSoundConsumer::MissionEnd,
        ),
    ];
    let mut parse_sites: BTreeSet<u32> = BTreeSet::new();
    let mut consumer_sites: BTreeSet<u32> = BTreeSet::new();
    for (key, consumer) in expected_consumers {
        let disposition = record_sound_disposition(key.key())
            .unwrap_or_else(|| panic!("{} is in the vocabulary", key.key()));
        let RecordSoundDisposition::Measured(measured) = disposition else {
            panic!(
                "{} is measured, not refused: {:?}",
                key.key(),
                disposition.refusal()
            );
        };
        assert_eq!(
            measured.consumer,
            consumer,
            "{}'s consumer is the measured one",
            key.key()
        );
        assert!(
            !measured.summary.is_empty()
                && !measured.evidence.is_empty()
                && !measured.unknowns.is_empty(),
            "{} names its effect, its evidence and the unknowns it leaves",
            key.key()
        );
        assert!(
            measured
                .evidence
                .contains(&"2026-10-09-m02-b-fu2-record-sound-keys"),
            "{} cites this task's finding",
            key.key()
        );
        assert!(
            parse_sites.insert(measured.parse_site),
            "parse site 0x{:x} is not shared between keys",
            measured.parse_site
        );
        assert!(
            consumer_sites.insert(measured.consumer_site),
            "consumer site 0x{:x} is not shared between keys",
            measured.consumer_site
        );
        assert_ne!(
            measured.field_offset,
            0,
            "{} names the mission-object field it is stored in",
            key.key()
        );
    }

    // What the record actually spells beside each key: one text value, the
    // sound-group name the original resolves to a handle.
    let sound_shape_rows = record.record_sound_shapes();
    let shapes: Vec<(&str, String)> = sound_shape_rows
        .iter()
        .map(|(key, shape)| (key.as_str(), shape.label()))
        .collect();
    assert_eq!(
        shapes,
        m02_sounds
            .iter()
            .map(|key| (key.key(), "[text]".to_owned()))
            .collect::<Vec<_>>(),
        "each sound key M02 spells carries exactly one text value: the sound-group name"
    );
}

/// **Each sound key parses where production says it parses and is consumed
/// where production says it is consumed — read out of the owner's decrypted
/// image, not out of the table.** The instruction bytes re-derive both
/// selectors: the objective-class `dec`/`je` chain (whose arms are the three
/// class consumer sites, in class order) and the mission-end won-flag
/// conditional (whose arm is the lost handle, the fall-through the won one).
/// A `parse_site`, `field_offset` or `consumer` in
/// `cs_content::mission_control` that drifts from the original fails here.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m02_b_fu2_the_image_parses_and_consumes_each_sound_key_where_production_says() {
    let image = engine_image();
    assert_eq!(
        sha256(&image).to_hex(),
        ORIGINAL_IMAGE_SHA256,
        "the image is the one every measurement here was read from"
    );

    // Every key: the parser's `push <string>` names this key, stores a zero
    // default, looks the child up by name, resolves that name to a handle and
    // writes the handle into the field production records.
    for key in CONTROL_RECORD_SOUND_KEY_VOCABULARY {
        let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key) else {
            panic!("{key} is in the vocabulary and measured");
        };
        let parse = file_offset(measured.parse_site);
        assert_eq!(
            byte_at(&image, parse),
            0x68,
            "{key}: the parse site is a `push imm32` of the key's string"
        );
        let string_va = u32_at(&image, parse + 1);
        string_at(&image, string_va, key);
        assert_eq!(
            [byte_at(&image, parse + 6), byte_at(&image, parse + 7)],
            [0x89, 0xbb],
            "{key}: the parser writes the zero default into the mission object"
        );
        assert_eq!(
            u32_at(&image, parse + 8),
            measured.field_offset,
            "{key}: the zero default lands in the recorded field"
        );
        assert_eq!(
            rel32_target(&image, parse + 0xc),
            0x57_a090,
            "{key}: the parse looks the key up in the record by name"
        );
        assert_eq!(
            rel32_target(&image, parse + 0x1f),
            0x59_6120,
            "{key}: the parse resolves the name through the name-to-handle lookup"
        );
        assert_eq!(
            [byte_at(&image, parse + 0x27), byte_at(&image, parse + 0x28)],
            [0x89, 0x83],
            "{key}: the resolved handle is stored into the mission object"
        );
        assert_eq!(
            u32_at(&image, parse + 0x29),
            measured.field_offset,
            "{key}: the handle lands in the recorded field"
        );

        // The consumer reads that same field back.
        let consume = file_offset(measured.consumer_site);
        assert_eq!(
            byte_at(&image, consume),
            0x8b,
            "{key}: the consumer loads the field"
        );
        let base = byte_at(&image, consume + 1);
        let expected_base = match measured.consumer {
            // `CZMission::Update` keeps `this` in ebx — the objective-class
            // chain and the end-of-tick outcome block both read through it;
            // the mission-end routine keeps it in esi.
            RecordSoundConsumer::ObjectiveCompletion { .. }
            | RecordSoundConsumer::ObjectivesOutcome { .. } => 0x83,
            RecordSoundConsumer::MissionEnd => 0x86,
        };
        assert_eq!(
            base, expected_base,
            "{key}: the consumer's addressing mode is the measured one"
        );
        assert_eq!(
            u32_at(&image, consume + 2),
            measured.field_offset,
            "{key}: the consumer reads the field the parser wrote"
        );
    }

    // Selector 1: the objective-class chain in `CZMission::Update`. Its arms
    // are the three class consumer sites, in class order — read from the
    // instruction bytes, so the class each key carries is checked against the
    // original rather than against itself.
    let dispatch = file_offset(0x46_a97d);
    assert_eq!(
        &image[dispatch..dispatch + 11],
        &[
            0x8b, 0x06, 0x48, 0x74, 0x1e, 0x48, 0x74, 0x0f, 0x48, 0x75, 0x2d
        ],
        "the completion tail dispatches on the objective's class field with the measured \
         `dec`/`je` chain"
    );
    let chain = 0x46_a97d;
    // arm 1: the first `je` (at chain+3); arm 2: the second `je` (chain+6);
    // arm 3: the fall-through of the final `jne` (chain+11).
    let class_1 = rel8_target(&image, chain + 3);
    let class_2 = rel8_target(&image, chain + 6);
    let class_3 = chain + 11;
    for (class, site) in [
        (1u8, class_1),
        (2, class_2),
        (3, class_3),
        // class 0 and every class past 3 fall through the whole chain
    ] {
        let key = match class {
            1 => ControlRecordSound::PrimaryComplete,
            2 => ControlRecordSound::SecondaryComplete,
            _ => ControlRecordSound::TertiaryComplete,
        };
        let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key.key())
        else {
            panic!("{} is measured", key.key());
        };
        assert_eq!(
            measured.consumer,
            RecordSoundConsumer::ObjectiveCompletion { class },
            "{} is selected by presentation class {class}",
            key.key()
        );
        assert_eq!(
            measured.consumer_site,
            site,
            "{} is the class-{class} arm of the measured chain",
            key.key()
        );
    }

    // Selector 2: the mission-end routine loads the won flag, and the
    // conditional chooses between the two handles.
    let flag_va = 0x46_3c49;
    let flag = file_offset(flag_va);
    assert_eq!(
        &image[flag..flag + 6],
        &[0x39, 0x8e, 0x58, 0x0c, 0x00, 0x00],
        "the mission-end routine compares the mission's won flag at +0xc58"
    );
    let branch_va = flag_va + 0x10; // `cmp`, then `mov [+0xc54], 1`
    let branch = file_offset(branch_va);
    assert_eq!(
        [byte_at(&image, branch), byte_at(&image, branch + 1)],
        [0x74, 0x2b],
        "the won test branches over the won handle"
    );
    let lost_site = rel8_target(&image, branch_va);
    let won_site = branch_va + 2;
    let Some(RecordSoundDisposition::Measured(won)) =
        record_sound_disposition(ControlRecordSound::MissionWon.key())
    else {
        panic!("MISSION_WON_SOUND is measured")
    };
    let Some(RecordSoundDisposition::Measured(lost)) =
        record_sound_disposition(ControlRecordSound::MissionLost.key())
    else {
        panic!("MISSION_LOST_SOUND is measured")
    };
    assert_eq!(
        won.consumer_site, won_site,
        "the won branch falls through to MISSION_WON_SOUND's field"
    );
    assert_eq!(
        lost.consumer_site, lost_site,
        "the conditional lands on MISSION_LOST_SOUND's field"
    );
    assert_eq!(
        won.consumer, lost.consumer,
        "both mission handles are consumed by the same routine"
    );
    assert_eq!(
        &image[file_offset(lost.consumer_site) + 6..file_offset(lost.consumer_site) + 8],
        &[0xeb, 0xd3],
        "the lost arm joins the won arm's play call instead of falling through"
    );
}

/// **The sound vocabulary and the disposition table answer for exactly each
/// other, in CI, without original data.** A key added to the vocabulary
/// without a measurement would be `Refused` and is caught here; a spelling the
/// table answers for outside the vocabulary would be read from its name and is
/// caught here too. When this test was written the vocabulary held M02's five
/// and the two `OBJECTIVES_*_SOUND` keys were the "real but unadmitted" arm;
/// RECORD-OBJECTIVES-SOUND (#808) has since admitted them — the arm now stands
/// on a key no original spells.
#[test]
fn accept_m02_b_fu2_the_sound_vocabulary_is_entirely_measured_and_answers_for_nothing_else() {
    let mut measured_count = 0;
    for key in CONTROL_RECORD_SOUND_KEY_VOCABULARY {
        let Some(disposition) = record_sound_disposition(key) else {
            panic!("{key} is in the vocabulary, so it has a disposition");
        };
        let RecordSoundDisposition::Measured(measured) = &disposition else {
            panic!(
                "{key} is measured, not refused: {:?}",
                disposition.refusal().map(|reason| reason.code())
            );
        };
        assert!(
            !measured.consumer.code().is_empty(),
            "{key}: the consumer carries a stable identifier"
        );
        assert!(
            measured.parse_site != measured.consumer_site,
            "{key}: the parse site and the consumer site are different places"
        );
        measured_count += 1;
    }
    assert_eq!(
        measured_count,
        CONTROL_RECORD_SOUND_KEY_VOCABULARY.len(),
        "every vocabulary key is measured"
    );

    for outside in [
        // Shape-measured record fields are not sound keys.
        "MISSION_TIMER",
        "PLAYER_INIT",
        "RESTORE_ANIMS",
        "EXECUTE_ANIMS",
        "INVALIDATE_ANIMS",
        // And a key no original spells.
        "A_SOUND_KEY_NOBODY_HAS_MEASURED",
    ] {
        assert!(
            record_sound_disposition(outside).is_none(),
            "{outside} has no sound disposition: the table answers for the vocabulary and \
             nothing else"
        );
    }

    // The consumers partition the seven keys the way the original does: three
    // objective completions with the classes 1..3, two mission ends, and two
    // objectives outcomes — one won, one lost (RECORD-OBJECTIVES-SOUND, #808).
    let classes: Vec<u8> = CONTROL_RECORD_SOUND_KEY_VOCABULARY
        .iter()
        .filter_map(|key| {
            let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            else {
                return None;
            };
            measured.consumer.class()
        })
        .collect();
    assert_eq!(classes, [1, 2, 3], "the three class handles carry classes");
    assert_eq!(
        CONTROL_RECORD_SOUND_KEY_VOCABULARY
            .iter()
            .filter(|key| {
                matches!(
                    record_sound_disposition(key),
                    Some(RecordSoundDisposition::Measured(measured))
                        if measured.consumer == RecordSoundConsumer::MissionEnd
                )
            })
            .count(),
        2,
        "the two mission handles are consumed at mission end"
    );
    let mut outcome_won = 0;
    let mut outcome_lost = 0;
    for key in CONTROL_RECORD_SOUND_KEY_VOCABULARY {
        if let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            && let RecordSoundConsumer::ObjectivesOutcome { won } = measured.consumer
        {
            if won {
                outcome_won += 1;
            } else {
                outcome_lost += 1;
            }
        }
    }
    assert_eq!(
        (outcome_won, outcome_lost),
        (1, 1),
        "the two objectives handles are consumed by the end-of-tick outcome block"
    );
}
