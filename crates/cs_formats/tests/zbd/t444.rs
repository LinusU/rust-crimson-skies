//! Acceptance for task #444: block decoding the two ADPCM layouts the retail
//! sound archives declare, by production code
//! (`cs_formats::zbd::SampleFormat::from_member`,
//! `cs_formats::zbd::decode_sound_sample`, `cs_formats::zbd::adpcm`)
//! (`docs/findings/2026-10-02-t444-ima-and-ms-adpcm-block-decoding.md`).
//!
//! The synthetic `accept_t444_*` tests author every byte here: RIFF/WAVE
//! members laid out as the Microsoft/IBM *Multimedia Programming Interface and
//! Data Specifications 1.0* documents (task #344's source), with `fmt `
//! extensions and blocks laid out as the two block layouts RFC 2361's
//! `wFormatTag` values name. No original game data, no `CS_GAME_DIR` access.
//!
//! The expected sample values are not read back from the decoder: each test
//! works them out from the format's own arithmetic (the step codebook, the
//! quarter-step weights, the coefficient weights and the adaptation table), so a
//! decoder that reorders a nibble, halves a weight, truncates instead of
//! clipping, or carries a table of its own fails here.
//!
//! The retail test reads the installation at `$CS_GAME_DIR` (never writes it)
//! and fails loudly without it. `evidence_report_t444_writes_the_acceptance_report`
//! is the evidence harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance
//! test; it also compares production decodes against FFmpeg's independent
//! decoders of the same two formats when `CS_FFMPEG` names one.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use cs_formats::ParseContext;
use cs_formats::zbd::{
    ADAPTATION_TABLE, AdpcmError, AdpcmExtension, AdpcmLayout, IMA_BLOCK_HEADER_BYTES, INDEX_TABLE,
    MAX_STEP_INDEX, MS_BLOCK_HEADER_BYTES_PER_CHANNEL, MsAdpcmCoefficient, MsAdpcmCoefficients,
    SAMPLE_VALUE_BYTES, STEP_TABLE, STEP_TABLE_ENTRIES, SampleError, SampleFormat,
    SampleFormatError, SampleLayout, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM,
    WaveHeader, decode_sound_sample, format_name, read_adpcm_extension, read_wave_header,
};

use super::t340::{
    artifact, command_output, env_var, game_dir, git, iso_utc_now, jstr, locked_version,
    parse_suite, path, retail_zbd_files, short_name, workspace_path,
};

/// Provenance label carried by every synthetic result.
const CONTAINER: &str = "synthetic/t444_adpcm.zbd";

/// The MS ADPCM coefficient table every retail member declares, in the order
/// `aCoefs` lists it. These are measured values, not this crate's invention: the
/// retail `fmt ` chunks carry exactly this table, and it is the table FFmpeg's
/// `libavcodec/adpcm_data.c` derives its own coefficient weights from.
const RETAIL_MS_COEFFICIENTS: [(i16, i16); 7] = [
    (256, 0),
    (512, -256),
    (0, 0),
    (192, 64),
    (240, 0),
    (460, -208),
    (392, -232),
];

/// A `fmt ` payload: the 16 common fields plus the tag's extension bytes.
fn fmt(tag: u16, channels: u16, rate_hz: u32, block_align: u16, tail: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate_hz.to_le_bytes());
    // `nAvgBytesPerSec` is not read here; the value is the conventional
    // rate * block size, which the retail members follow for PCM and for a
    // block codec alike it is a derived average.
    bytes.extend_from_slice(&(rate_hz * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    // Both block layouts store four bits per sample; PCM declares its own width.
    let bits: u16 = if tag == WAVE_FORMAT_PCM { 16 } else { 4 };
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(tail);
    bytes
}

/// The IMA ADPCM `fmt ` extension: `cbSize` 2 and `wSamplesPerBlock`.
fn ima_tail(samples_per_block: u16) -> Vec<u8> {
    let mut bytes = 2u16.to_le_bytes().to_vec();
    bytes.extend_from_slice(&samples_per_block.to_le_bytes());
    bytes
}

/// The MS ADPCM `fmt ` extension: `cbSize` 32, `wSamplesPerBlock`, `wNumCoefs`
/// and that many coefficient pairs.
fn ms_tail(samples_per_block: u16, pairs: &[(i16, i16)]) -> Vec<u8> {
    let mut bytes = 32u16.to_le_bytes().to_vec();
    bytes.extend_from_slice(&samples_per_block.to_le_bytes());
    bytes.extend_from_slice(&(pairs.len() as u16).to_le_bytes());
    for (predictor, difference) in pairs {
        bytes.extend_from_slice(&predictor.to_le_bytes());
        bytes.extend_from_slice(&difference.to_le_bytes());
    }
    bytes
}

/// A complete RIFF/WAVE member around a `fmt ` payload and a `data` payload.
fn member(fmt_payload: &[u8], data: &[u8]) -> Vec<u8> {
    let mut chunks = Vec::new();
    for (id, payload) in [(b"fmt ", fmt_payload), (b"data", data)] {
        chunks.extend_from_slice(id);
        chunks.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunks.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            chunks.push(0);
        }
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&chunks);
    bytes
}

/// An IMA ADPCM block: the documented four header bytes, then the nibbles.
fn ima_block(predictor: i16, step_index: u8, nibbles: &[u8]) -> Vec<u8> {
    assert_eq!(
        nibbles.len() % 2,
        0,
        "an IMA block stores two nibbles per byte"
    );
    let mut bytes = predictor.to_le_bytes().to_vec();
    bytes.push(step_index);
    bytes.push(0);
    for pair in nibbles.as_chunks::<2>().0 {
        debug_assert!(pair[1] < 16, "a nibble is four bits");
        bytes.push(pair[0] | (pair[1] << 4));
    }
    bytes
}

/// An MS ADPCM block: seven header bytes per channel, grouped by field, then the
/// nibbles, high one first.
fn ms_block(
    indices: &[u8],
    deltas: &[i16],
    firsts: &[i16],
    seconds: &[i16],
    nibbles: &[u8],
) -> Vec<u8> {
    let channels = indices.len();
    assert_eq!(
        (deltas.len(), firsts.len(), seconds.len()),
        (channels, channels, channels)
    );
    let mut bytes = indices.to_vec();
    for field in [deltas, firsts, seconds] {
        for value in field {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    if channels == 1 {
        // A mono byte holds the block's next two samples, high nibble first.
        for pair in nibbles.as_chunks::<2>().0 {
            assert!(pair[1] < 16, "a nibble is four bits");
            bytes.push((pair[0] << 4) | pair[1]);
        }
    } else {
        // A two-channel byte holds the first channel's next sample in its high
        // nibble and the second's in the low one.
        for group in nibbles.chunks_exact(channels) {
            for nibble in group {
                assert!(*nibble < 16, "a nibble is four bits");
                bytes.push(nibble << 4);
            }
        }
    }
    bytes
}

/// The plan a member's own bytes declare, read by production code.
fn plan(bytes: &[u8]) -> SampleFormat {
    SampleFormat::from_member(bytes).expect("the member declares a decodable format")
}

/// The plan and the decode of one authored member.
fn decode(bytes: &[u8]) -> (SampleFormat, cs_formats::zbd::DecodedSound) {
    let declared = plan(bytes);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let decoded = decode_sound_sample(&mut context, bytes, &declared).expect("the member decodes");
    (declared, decoded)
}

// --- IMA ADPCM (wFormatTag 0x0011)

#[test]
fn accept_t444_an_ima_adpcm_member_decodes_the_blocks_its_header_declares() {
    // A mono member at `nBlockAlign` 8, so a block holds the four header bytes
    // and two nibble bytes: `1 + 2 * (8 - 4) = 9` samples, which is what the
    // `fmt ` extension declares. Two blocks, so the sample count is 18.
    //
    // The first block's nibbles are 1, 2, 3, 4, 5, 6, 7, 8, low nibble first, and
    // its step index starts at 0, where the codebook step is 7. A nibble always
    // moves the predictor by `step / 8` plus `step / 4`, `step / 2` and `step` for
    // each magnitude bit that is set, subtracted when bit 3 is set, and then
    // moves the step index by the index table: -1 for the small nibbles, +2, +4,
    // +6 or +8 for the larger ones, clamped to the codebook. So from 1000 at
    // step 7, with the step index rising 0, 0, 0, 2, 6, 12, 20 as the nibbles grow
    // and the steps with it: +1, +3, +4, +7, +12, +20, +41, and -6 for the nibble
    // 8, which sets the sign bit and no magnitude bit.
    let block = ima_block(1000, 0, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &[block.clone(), block].concat(),
    );
    let (declared, decoded) = decode(&bytes);

    // The plan is the member's own declaration.
    assert_eq!(
        declared.layout(),
        SampleLayout::Adpcm(AdpcmLayout::Ima {
            samples_per_block: 9
        })
    );
    assert!(declared.layout().is_block_coded());
    assert_eq!(declared.layout().pcm(), None);
    assert_eq!(
        declared.layout().adpcm().map(|layout| layout.format_tag()),
        Some(WAVE_FORMAT_IMA_ADPCM)
    );
    assert_eq!(declared.channels(), 1);
    assert_eq!(declared.rate_hz(), 11_025);
    assert_eq!(declared.block_align(), 8);
    assert_eq!(declared.frame_bytes(), 8, "a frame is a whole block");
    assert_eq!(declared.samples_per_frame(), 9);
    assert_eq!(declared.data_span().length, 16, "two eight-byte blocks");

    // The counts are the ones the declared block geometry implies.
    assert_eq!(decoded.frames(), 2, "two blocks");
    assert_eq!(decoded.sample_count(), 18);
    assert_eq!(decoded.byte_len(), 16);
    assert_eq!(decoded.samples_per_frame(), 9);
    assert_eq!(decoded.samples().len(), 18);

    // The values are the ones the format's own arithmetic gives, and the second
    // block restates its predictor instead of continuing the first: every block
    // is decoded from its own header.
    let ramp: Vec<i32> = [1000, 1001, 1004, 1008, 1015, 1027, 1047, 1088, 1082]
        .into_iter()
        .collect();
    let expected: Vec<i32> = ramp.iter().chain(ramp.iter()).copied().collect();
    assert_eq!(decoded.samples(), expected.as_slice());

    // One frame is one block, so the frame accessor splits the values the same
    // way.
    assert_eq!(decoded.frame(0), Some(ramp.as_slice()));
    assert_eq!(decoded.frame(1), Some(ramp.as_slice()));
    assert_eq!(decoded.frame(2), None);
}

#[test]
fn accept_t444_an_ima_block_saturates_by_clipping_and_clamps_its_step_index() {
    // A block whose step index is the codebook's last entry, whose step is 32767
    // and whose nibbles drive the predictor past the 16-bit range. The value is
    // clipped, never wrapped: -32768 is the low end of the format's own range.
    // Nibble 15 sets every magnitude bit, so one nibble moves the predictor by
    // 32767/8 + 32767/4 + 32767/2 + 32767 = 57341, and nibble 7 is the same
    // magnitude with the opposite sign.
    let block = ima_block(20_000, MAX_STEP_INDEX, &[15, 7]);
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &block,
    );
    let (_, decoded) = decode(&bytes);
    // 20000 - 61436 clips to -32768, and -32768 + 61436 is 28668. The step index
    // stays at the codebook's end: the index table would raise it by 8, and the
    // clamp is what keeps the step at 32767.
    assert_eq!(decoded.samples(), &[20_000, -32_768, 28_668]);

    // A step index past the codebook is a corrupt header, not a value to clamp.
    let mut corrupt = ima_block(0, 0, &[0, 0, 0, 0, 0, 0, 0, 0]);
    corrupt[2] = STEP_TABLE_ENTRIES as u8;
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &corrupt,
    );
    let declared = plan(&bytes);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_sound_sample(&mut context, &bytes, &declared)
        .expect_err("a step index past the codebook is not a sample");
    assert_eq!(error.code(), "adpcm_step_index_out_of_range");
    assert_eq!(
        error,
        SampleError::Block(AdpcmError::StepIndexOutOfRange {
            offset: 0,
            step_index: STEP_TABLE_ENTRIES as u8,
        })
    );
    assert_eq!(
        context.allocation().used(),
        0,
        "a refused block charges nothing"
    );
}

// --- MS ADPCM (wFormatTag 0x0002)

#[test]
fn accept_t444_an_ms_adpcm_member_decodes_the_blocks_its_header_declares() {
    // A mono member at `nBlockAlign` 8: seven header bytes and one nibble byte,
    // so a block holds the two history samples plus the byte's two samples,
    // `2 + 2 * (8 - 7) = 4`, which is what the `fmt ` extension declares.
    //
    // The block names coefficient pair 0, which the member's own table declares
    // as (256, 0), so the predictor is the newer history sample unchanged; the
    // delta starts at 100 and the byte carries the nibbles 5 and 3. The two
    // history values are output in the order the layout stores them, the older
    // one first.
    let block = ms_block(&[0], &[100], &[2000], &[1000], &[5, 3]);
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            1,
            22_050,
            8,
            &ms_tail(4, &RETAIL_MS_COEFFICIENTS),
        ),
        &block,
    );
    let (declared, decoded) = decode(&bytes);

    let Some(AdpcmLayout::Ms { coefficients, .. }) = declared.layout().adpcm() else {
        panic!("expected the MS layout, got {:?}", declared.layout())
    };
    assert_eq!(
        declared.layout(),
        SampleLayout::Adpcm(AdpcmLayout::Ms {
            samples_per_block: 4,
            coefficients,
        })
    );
    assert_eq!(coefficients.count(), 7);
    assert_eq!(
        declared.layout().adpcm().map(|layout| layout.format_tag()),
        Some(WAVE_FORMAT_MS_ADPCM)
    );
    assert_eq!(declared.channels(), 1);
    assert_eq!(declared.rate_hz(), 22_050);
    assert_eq!(declared.block_align(), 8);
    assert_eq!(declared.samples_per_frame(), 4);
    assert_eq!(decoded.frames(), 1);
    assert_eq!(decoded.sample_count(), 4);
    assert_eq!(decoded.byte_len(), 8);

    // 1000 and 2000 are the block's two history values; the nibble 5 predicts
    // 2000 + 5 * 100 = 2500 and adapts the delta to 409 * 100 / 256 = 159; the
    // nibble 3 then predicts 2500 + 3 * 159 = 2977.
    assert_eq!(decoded.samples(), &[1000, 2000, 2500, 2977]);
    assert_eq!(decoded.frame(0), Some(&[1000, 2000, 2500, 2977][..]));
    assert_eq!(decoded.frame(1), None);
}

#[test]
fn accept_t444_a_stereo_ms_adpcm_block_decodes_one_sample_per_channel_in_order() {
    // A stereo member at `nBlockAlign` 16: fourteen header bytes and one nibble
    // byte, so a block holds `2 + 2 * ((16 - 14) / 2) = 4` samples per channel,
    // eight values in all. The byte's high nibble is the first channel's next
    // sample and the low nibble the second's, and a block's values are stored
    // channel by channel: both older history samples, then both newer ones, then
    // the byte.
    let block = ms_block(&[0, 1], &[100, 200], &[2000, -2000], &[1000, 500], &[5, 3]);
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            2,
            22_050,
            16,
            &ms_tail(4, &RETAIL_MS_COEFFICIENTS),
        ),
        &block,
    );
    let (declared, decoded) = decode(&bytes);
    assert_eq!(declared.channels(), 2);
    assert_eq!(
        declared.samples_per_frame(),
        8,
        "four samples for each of two channels"
    );
    assert_eq!(decoded.frames(), 1);
    assert_eq!(decoded.sample_count(), 8);

    // The first nibble byte is 0x50: the first channel's nibble 5 and the
    // second's nibble 0. Channel 0 uses pair (256, 0), so it predicts its newer
    // history sample, 2000 + 5 * 100 = 2500, and adapts its delta to
    // 409 * 100 / 256 = 159. Channel 1 uses pair (512, -256), so it predicts
    // (-2000 * 512 + 500 * -256) / 256 = -4500 and moves by nothing. The second
    // byte is 0x30: channel 0's nibble 3 gives 2500 + 3 * 159 = 2977, and channel
    // 1's nibble 0 gives (-4500 * 512 + -2000 * -256) / 256 = -7000.
    assert_eq!(
        decoded.samples(),
        &[1000, 500, 2000, -2000, 2500, -4500, 2977, -7000],
        "channel by channel, in the order the block stores them"
    );
    assert_eq!(
        decoded.frame(0),
        Some(&[1000, 500, 2000, -2000, 2500, -4500, 2977, -7000][..])
    );
}

#[test]
fn accept_t444_the_block_decode_uses_the_coefficients_the_member_declares() {
    // The same block, decoded under a member whose own `fmt ` table is not the
    // retail one: pair 0 is (128, 64) here, so the predictor is
    // (2000 * 128 + 1000 * 64) / 256 = 1250 and the nibble 5 gives 1750. A
    // decoder carrying a table of its own would give 2500 here instead, the value
    // the retail table produces.
    let block = ms_block(&[0], &[100], &[2000], &[1000], &[5, 0]);
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            1,
            22_050,
            8,
            &ms_tail(4, &[(128, 64), (512, -256)]),
        ),
        &block,
    );
    let (declared, decoded) = decode(&bytes);
    let Some(AdpcmLayout::Ms { coefficients, .. }) = declared.layout().adpcm() else {
        panic!("expected the MS layout, got {:?}", declared.layout())
    };
    assert_eq!(coefficients.count(), 2, "the member declares two pairs");
    assert_eq!(coefficients.get(0), Some(MsAdpcmCoefficient::new(128, 64)));
    assert_eq!(
        coefficients.get(1),
        Some(MsAdpcmCoefficient::new(512, -256))
    );
    assert_eq!(
        coefficients.get(2),
        None,
        "an index past the member's own table is not answered from elsewhere"
    );
    // The nibble 0 still predicts from the two history values, which are now
    // 1750 and 2000: (1750 * 128 + 2000 * 64) / 256 = 1375.
    assert_eq!(decoded.samples(), &[1000, 2000, 1750, 1375]);

    // A member cannot declare more coefficient pairs than the layout reserves.
    let table = MsAdpcmCoefficients::new([MsAdpcmCoefficient::new(1, 2); 7], 9);
    assert_eq!(
        table.count(),
        7,
        "the count is clamped to the layout's seven"
    );

    // A block that names pair 2 of a two-pair table is refused, naming the
    // member's own count.
    let block = ms_block(&[2], &[100], &[2000], &[1000], &[0, 0]);
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            1,
            22_050,
            8,
            &ms_tail(4, &[(128, 64), (512, -256)]),
        ),
        &block,
    );
    let declared = plan(&bytes);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_sound_sample(&mut context, &bytes, &declared)
        .expect_err("a coefficient the member does not declare is not decoded");
    assert_eq!(error.code(), "adpcm_coefficient_out_of_range");
    assert_eq!(
        error,
        SampleError::Block(AdpcmError::CoefficientOutOfRange {
            offset: 0,
            channel: 0,
            index: 2,
            declared: 2,
        })
    );
    assert_eq!(context.allocation().used(), 0);
    assert!(error.to_string().contains('2'));
}

#[test]
fn accept_t444_the_format_codebooks_are_the_ones_the_two_layouts_name() {
    // The step, index and adaptation codebooks are the formats' own tables, not
    // this crate's tuning: they are what implementations of these two layouts
    // carry, and the values below are what a member's samples move by. Pinning
    // them here means a table edited in `adpcm.rs` fails this test rather than
    // quietly changing every decoded sample.
    assert_eq!(STEP_TABLE.len(), 89);
    assert_eq!(STEP_TABLE[0], 7, "the smallest step");
    assert_eq!(STEP_TABLE[10], 19);
    assert_eq!(STEP_TABLE[44], 494);
    assert_eq!(STEP_TABLE[88], 32_767, "the largest step");
    assert!(
        STEP_TABLE.windows(2).all(|pair| pair[0] < pair[1]),
        "the step codebook rises"
    );
    assert_eq!(MAX_STEP_INDEX as usize, STEP_TABLE.len() - 1);

    assert_eq!(
        INDEX_TABLE,
        [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8],
        "a nibble's step-index movement, indexed by the nibble"
    );
    assert_eq!(
        ADAPTATION_TABLE,
        [
            230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230
        ]
    );
    assert_eq!(
        IMA_BLOCK_HEADER_BYTES, 4,
        "predictor, step index, one reserved byte"
    );
    assert_eq!(MS_BLOCK_HEADER_BYTES_PER_CHANNEL, 7);
    assert_eq!(SAMPLE_VALUE_BYTES, 4, "one decoded value is an i32");
}

// --- A trailing short block, refusals, and the budget

#[test]
fn accept_t444_a_trailing_short_block_is_decoded_by_the_documented_geometry() {
    // No retail member ends in a short block, so this case follows the layouts'
    // own geometry: a final block of five bytes holds the four-byte IMA header and
    // one nibble byte, which is `1 + 2 * 1 = 3` samples. It is decoded, not
    // refused and not padded to a whole block.
    let first = ima_block(100, 0, &[1, 1, 1, 1, 1, 1, 1, 1]);
    let last = ima_block(500, 0, &[1, 1]);
    assert_eq!(first.len(), 8);
    assert_eq!(last.len(), 5);
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &[first.clone(), last].concat(),
    );
    let (declared, decoded) = decode(&bytes);
    assert_eq!(decoded.frames(), 2);
    assert_eq!(decoded.byte_len(), 13, "the payload's own length");
    assert_eq!(
        decoded.sample_count(),
        12,
        "nine samples from the full block, three from the short one"
    );
    assert_eq!(decoded.samples()[9], 500, "the short block's own predictor");
    assert_eq!(decoded.samples().len(), 12);
    // The geometry count and the decode agree, which is what the booking check
    // enforces: the count booked against the budget is the one the decode hits.
    let counted = declared
        .layout()
        .adpcm()
        .expect("a block-coded plan")
        .sample_count(declared.channels(), declared.frame_bytes(), 13)
        .expect("the geometry reads");
    assert_eq!(counted, decoded.sample_count());

    // A final block too short for even its own header is refused, and the
    // reservation is rolled back.
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &[first, vec![0u8; 3]].concat(),
    );
    let declared = plan(&bytes);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_sound_sample(&mut context, &bytes, &declared)
        .expect_err("a block that cannot hold its header is not a sample");
    assert_eq!(error.code(), "adpcm_short_block");
    assert_eq!(
        error,
        SampleError::Block(AdpcmError::ShortBlock {
            offset: 8,
            bytes: 3,
            header: IMA_BLOCK_HEADER_BYTES,
        })
    );
    assert_eq!(error.offset(), 8);
    assert_eq!(context.allocation().used(), 0, "the charge is rolled back");
}

#[test]
fn accept_t444_a_stereo_block_without_whole_channel_groups_is_refused() {
    // A stereo block needs whole channel groups of nibble bytes: one byte holds
    // the first channel's sample in its high nibble and the second's in the low
    // one, so an odd byte count would leave a channel without its share. That is
    // refused rather than half-decoded.
    let mut block = ms_block(&[0, 0], &[100, 100], &[10, 10], &[0, 0], &[0, 0]);
    assert_eq!(
        block.len(),
        16,
        "fourteen header bytes and one byte per channel"
    );
    block.pop();
    assert_eq!(
        block.len(),
        15,
        "one nibble byte short of a whole channel group"
    );
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            2,
            22_050,
            16,
            &ms_tail(4, &RETAIL_MS_COEFFICIENTS),
        ),
        &block,
    );
    let declared = plan(&bytes);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_sound_sample(&mut context, &bytes, &declared)
        .expect_err("half a channel group is not a sample");
    assert_eq!(error.code(), "adpcm_partial_block");
    assert_eq!(
        error,
        SampleError::Block(AdpcmError::PartialBlock {
            offset: 0,
            bytes: 1,
            channels: 2,
        })
    );
    assert_eq!(context.allocation().used(), 0);
}

#[test]
fn accept_t444_a_compressed_member_without_a_readable_fmt_extension_is_refused() {
    // A member that declares MS ADPCM and stops after `cbSize` has no
    // `wSamplesPerBlock` to read, so no block layout can be built from it. The
    // refusal carries the member's own tag and the two lengths; the header itself
    // still reads, because task #344 keeps the tail inside `fmt_span` without
    // interpreting it.
    let bytes = member(
        &fmt(WAVE_FORMAT_MS_ADPCM, 1, 22_050, 8, &32u16.to_le_bytes()),
        &[0u8; 8],
    );
    let header = read_wave_header(&bytes).expect("the header reads");
    assert_eq!(header.format_tag(), WAVE_FORMAT_MS_ADPCM);
    assert_eq!(header.fmt_span().length, 18);
    let error = SampleFormat::from_member(&bytes)
        .expect_err("a member without its extension has no block layout");
    assert_eq!(error.code(), "adpcm_extension_short");
    assert_eq!(
        error,
        SampleFormatError::AdpcmExtensionShort {
            tag: WAVE_FORMAT_MS_ADPCM,
            declared_len: 18,
            needed: 22,
        }
    );
    let text = error.to_string();
    assert!(text.contains("0x0002"), "{text}");
    assert!(text.contains("18"), "{text}");

    // The extension reader itself reports the same thing, so a caller holding
    // the header and the `fmt ` payload sees it without decoding anything.
    assert_eq!(
        read_adpcm_extension(WAVE_FORMAT_MS_ADPCM, &bytes[20..38]),
        AdpcmExtension::Short {
            tag: WAVE_FORMAT_MS_ADPCM,
            declared_len: 18,
            needed: 22,
        }
    );

    // An IMA member with no extension bytes at all is `Absent`, which the plan
    // refuses as an unsupported format carrying the member's own tag: this crate
    // reads that tag, and what is missing is the value that locates its blocks.
    let bytes = member(&fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &[]), &[0u8; 8]);
    assert_eq!(
        read_adpcm_extension(WAVE_FORMAT_IMA_ADPCM, &bytes[20..36]),
        AdpcmExtension::Absent
    );
    let error =
        SampleFormat::from_member(&bytes).expect_err("an IMA member with no `wSamplesPerBlock`");
    assert_eq!(
        error,
        SampleFormatError::UnsupportedFormat {
            tag: WAVE_FORMAT_IMA_ADPCM,
            name: format_name(WAVE_FORMAT_IMA_ADPCM),
        }
    );
    assert_eq!(error.code(), "unsupported_format");

    // A tag this crate does not decode is refused the same way, with the tag.
    let bytes = member(&fmt(0x0031, 1, 22_050, 8, &[]), &[0u8; 8]);
    let error = SampleFormat::from_member(&bytes).expect_err("an unnamed tag is not decoded");
    assert_eq!(
        error,
        SampleFormatError::UnsupportedFormat {
            tag: 0x0031,
            name: None
        }
    );
}

#[test]
fn accept_t444_a_declaration_the_block_layouts_cannot_honour_is_refused() {
    // `wSamplesPerBlock` that the declared block size cannot hold: a
    // contradiction inside one header, refused with both numbers.
    let block = ima_block(0, 0, &[0, 0, 0, 0, 0, 0, 0, 0]);
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(11)),
        &block,
    );
    let error =
        SampleFormat::from_member(&bytes).expect_err("a block of 8 bytes does not hold 11 samples");
    assert_eq!(error.code(), "samples_per_block_mismatch");
    assert_eq!(
        error,
        SampleFormatError::SamplesPerBlockMismatch {
            declared: 11,
            declared_block_align: 8,
            implied: 9,
        }
    );
    let text = error.to_string();
    assert!(text.contains("11"), "{text}");
    assert!(text.contains("9"), "{text}");

    // A block size that cannot hold even the layout's block header.
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 3, &ima_tail(1)),
        &[0u8; 3],
    );
    let error = SampleFormat::from_member(&bytes).expect_err("three bytes hold no block header");
    assert_eq!(
        error,
        SampleFormatError::BlockAlignTooSmall {
            declared: 3,
            minimum: IMA_BLOCK_HEADER_BYTES,
        }
    );

    // A block layout that stores four bits per sample at any other width is a
    // layout this crate has not read.
    let mut wide = fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9));
    wide[14..16].copy_from_slice(&8u16.to_le_bytes());
    let bytes = member(&wide, &block);
    let error = SampleFormat::from_member(&bytes).expect_err("8 bits per sample is not IMA");
    assert_eq!(
        error,
        SampleFormatError::UnsupportedWidth { bits_per_sample: 8 }
    );

    // A channel count no retail member declares is refused rather than guessed at:
    // no retail IMA member has more than one channel, and the layouts documented
    // for wider MS ADPCM blocks differ.
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 2, 11_025, 16, &ima_tail(25)),
        &[0u8; 16],
    );
    let error = SampleFormat::from_member(&bytes).expect_err("no retail IMA member is stereo");
    assert_eq!(error.code(), "adpcm_channels_not_observed");
    assert_eq!(
        error,
        SampleFormatError::AdpcmChannelsNotObserved {
            layout: "ima_adpcm",
            channels: 2,
        }
    );
    assert!(error.to_string().contains("ima_adpcm"));

    // Three MS ADPCM channels: mono and stereo are what the retail archives
    // declare, and a wider block is a different documented layout.
    let mut wide_block = vec![0u8; 3];
    for field in 0..3i16 {
        for _ in 0..3 {
            wide_block.extend_from_slice(&field.to_le_bytes());
        }
    }
    wide_block.extend_from_slice(&[0u8; 2]);
    let bytes = member(
        &fmt(
            WAVE_FORMAT_MS_ADPCM,
            3,
            22_050,
            25,
            &ms_tail(4, &RETAIL_MS_COEFFICIENTS),
        ),
        &wide_block,
    );
    let error = SampleFormat::from_member(&bytes).expect_err("three-channel MS ADPCM is not read");
    assert_eq!(
        error,
        SampleFormatError::AdpcmChannelsNotObserved {
            layout: "ms_adpcm",
            channels: 3,
        }
    );
    assert!(error.to_string().contains('3'));
}

#[test]
fn accept_t444_the_block_decode_is_bounded_by_the_parse_allocation_budget() {
    // The sample buffer is booked from the block geometry before it exists, so a
    // starved context refuses a compressed member without allocating, the refusal
    // is scoped `zbd.sample.sample.values`, and the retry on a funded context
    // decodes the very same member.
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &ima_block(0, 0, &[0, 0, 0, 0, 0, 0, 0, 0]),
    );
    let declared = plan(&bytes);
    let exact = 9 * SAMPLE_VALUE_BYTES;

    let mut starved = ParseContext::new(CONTAINER, exact - 1, 32);
    let error = decode_sound_sample(&mut starved, &bytes, &declared)
        .expect_err("a budget one byte short of the exact charge is refused");
    assert_eq!(error.code(), "allocation_budget_exceeded");
    let SampleError::Parse(parse) = &error else {
        panic!("expected a parse failure, got {error:?}")
    };
    assert_eq!(parse.field, "zbd.sample.sample.values");
    assert_eq!(parse.observed, format!("{exact} bytes requested"));
    assert_eq!(
        starved.allocation().used(),
        0,
        "the refused charge is rolled back"
    );
    assert_eq!(starved.recursion().depth(), 0);

    let mut funded = ParseContext::new(CONTAINER, exact, 32);
    let decoded =
        decode_sound_sample(&mut funded, &bytes, &declared).expect("the exact charge fits");
    assert_eq!(decoded.sample_count(), 9);
    assert_eq!(funded.allocation().used(), exact);
}

#[test]
fn accept_t444_the_pcm_entry_point_still_refuses_a_compressed_member_with_its_tag() {
    // Stage F06-C's entry point decodes uncompressed PCM only and says so with
    // the member's own tag; task #444's entry point decodes the same member. The
    // staged split is deliberate: it keeps the F06-C consumer's contract
    // (`SoundReadiness::UnsupportedFormat` for a compressed member) true until
    // that consumer is switched over.
    let bytes = member(
        &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 8, &ima_tail(9)),
        &ima_block(7, 0, &[0, 0, 0, 0, 0, 0, 0, 0]),
    );
    let header: WaveHeader = read_wave_header(&bytes).expect("the header reads");
    let error = SampleFormat::from_header(&header)
        .expect_err("the F06-C entry point does not decode a block codec");
    assert_eq!(
        error,
        SampleFormatError::UnsupportedFormat {
            tag: WAVE_FORMAT_IMA_ADPCM,
            name: Some("ima_adpcm"),
        }
    );
    assert_eq!(error.code(), "unsupported_format");

    // The same member through the block-aware entry point.
    let (_, decoded) = decode(&bytes);
    assert_eq!(decoded.sample_count(), 9);
    assert_eq!(decoded.samples()[0], 7, "the block's own predictor");

    // An unreadable member is refused by the block-aware entry point with the
    // header reader's own reason, before any block arithmetic.
    let not_riff = b"NOTRIFFxx not a wave file at all".to_vec();
    let wave_error = read_wave_header(&not_riff).expect_err("not a RIFF file");
    let from_member = SampleFormat::from_member(&not_riff).expect_err("the header does not read");
    assert_eq!(from_member.code(), "unreadable_header");
    assert_eq!(
        from_member,
        SampleFormatError::UnreadableHeader {
            reason: wave_error.reason()
        }
    );
    assert_eq!(from_member.reason(), Some(wave_error.reason()));
}

// --- Retail -------------------------------------------------------------------

/// One distinct compressed shape a retail member declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    tag: u16,
    channels: u16,
    rate_hz: u32,
    block_align: u16,
    samples_per_block: u16,
    coefficient_pairs: u16,
}

impl Shape {
    /// The shape as a JSON object, with no original bytes in it.
    fn json(&self) -> String {
        format!(
            "{{\"format_tag\": {}, \"format\": {}, \"channels\": {}, \"rate_hz\": {}, \
             \"block_align\": {}, \"samples_per_block\": {}, \"coefficient_pairs\": {}}}",
            self.tag,
            format_name(self.tag).map_or_else(|| "null".to_owned(), jstr),
            self.channels,
            self.rate_hz,
            self.block_align,
            self.samples_per_block,
            self.coefficient_pairs
        )
    }
}

/// One compressed member's measured facts, from production code only.
#[derive(Debug)]
struct RetailMember {
    spelling: String,
    name: String,
    shape: Shape,
    blocks: u64,
    samples: u64,
    bytes: u64,
    saturated: u64,
    /// The member's bytes, read once for the shape sweep and dropped again.
    content: Vec<u8>,
}

/// What production code decoded from every compressed member of both archives.
fn retail_adpcm() -> Vec<RetailMember> {
    let game = game_dir();
    let mut rows = Vec::new();
    for (spelling, host) in retail_zbd_files(&game) {
        let relative = path(&spelling);
        let bytes = fs::read(&host).unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        let probe = &bytes[..bytes.len().min(64)];
        let decided =
            cs_formats::zbd::dispatch(cs_formats::zbd::ZbdProbe::new(&spelling, &relative, probe))
                .unwrap_or_else(|error| panic!("{spelling} must dispatch: {error}"));
        if decided.family() != cs_formats::zbd::ZbdFamily::Sound {
            continue;
        }
        let mut context = ParseContext::with_defaults(spelling.clone());
        let index = cs_formats::zbd::read_version_one_index(&mut context, decided, &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: {error}"));
        let table = index.member_table();
        let sound = cs_formats::zbd::read_sound_archive(&mut context, &table, index.data())
            .unwrap_or_else(|error| panic!("{spelling}: {error}"));
        for entry in sound.entries() {
            let header = entry
                .wave()
                .expect("every retail member is a readable WAVE file");
            if header.format_tag() != WAVE_FORMAT_MS_ADPCM
                && header.format_tag() != WAVE_FORMAT_IMA_ADPCM
            {
                continue;
            }
            let name = String::from_utf8_lossy(entry.name()).into_owned();
            let declared = SampleFormat::from_member(entry.content())
                .unwrap_or_else(|error| panic!("{spelling}:{name}: {error}"));
            let Some(layout) = declared.layout().adpcm() else {
                panic!("{spelling}:{name}: a compressed member is a block codec")
            };
            // Each member is decoded on its own parse: a shared context would
            // accumulate every member's charge, and the whole retail corpus is
            // far larger than one parse's allocation budget.
            let mut member_context = ParseContext::with_defaults(format!("{spelling}:{name}"));
            let decoded = decode_sound_sample(&mut member_context, entry.content(), &declared)
                .unwrap_or_else(|error| panic!("{spelling}:{name}: {error}"));
            let saturated = decoded
                .samples()
                .iter()
                .filter(|value| **value == i32::from(i16::MAX) || **value == i32::from(i16::MIN))
                .count() as u64;
            rows.push(RetailMember {
                spelling: spelling.clone(),
                name,
                shape: Shape {
                    tag: header.format_tag(),
                    channels: header.channels(),
                    rate_hz: header.rate_hz(),
                    block_align: header.block_align(),
                    samples_per_block: layout.declared_samples_per_block(),
                    coefficient_pairs: match layout {
                        AdpcmLayout::Ms { coefficients, .. } => coefficients.count(),
                        AdpcmLayout::Ima { .. } => 0,
                    },
                },
                blocks: decoded.frames(),
                samples: decoded.sample_count(),
                bytes: decoded.byte_len(),
                saturated,
                content: entry.content().to_vec(),
            });
        }
    }
    rows
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t444_retail_every_compressed_sound_member_decodes_to_its_declared_counts() {
    let rows = retail_adpcm();
    // Task #344 counted 4,464 MS ADPCM members (1,954 in `soundsl.zbd` and 2,510
    // in `soundsh.zbd`) and 555 IMA ADPCM members (all in `soundsl.zbd`); every
    // one of them decodes here.
    assert_eq!(rows.len(), 5_019, "every compressed retail member decodes");
    let ima = rows
        .iter()
        .filter(|row| row.shape.tag == WAVE_FORMAT_IMA_ADPCM)
        .count();
    let ms = rows
        .iter()
        .filter(|row| row.shape.tag == WAVE_FORMAT_MS_ADPCM)
        .count();
    assert_eq!((ima, ms), (555, 4_464));

    for row in &rows {
        let where_ = format!("{}:{}", row.spelling, row.name);
        // The declared block geometry is what the payload holds, and the sample
        // count is blocks x samples per block x channels.
        assert_eq!(
            row.blocks * u64::from(row.shape.block_align),
            row.bytes,
            "{where_}"
        );
        assert_eq!(
            row.samples,
            row.blocks * u64::from(row.shape.samples_per_block) * u64::from(row.shape.channels),
            "{where_}"
        );
        assert!(row.samples > 0, "{where_}: a decoded member holds samples");
    }

    // Every MS ADPCM member declares the same seven coefficient pairs, so the
    // decode reads the member's own table rather than a table of this crate's,
    // and the IMA members declare none because the layout has none.
    for row in &rows {
        let expected = if row.shape.tag == WAVE_FORMAT_MS_ADPCM {
            7
        } else {
            0
        };
        assert_eq!(
            row.shape.coefficient_pairs, expected,
            "{}:{}",
            row.spelling, row.name
        );
    }

    // The distinct shapes the retail archives declare, as measured.
    let mut shapes: BTreeMap<Shape, usize> = BTreeMap::new();
    for row in &rows {
        *shapes.entry(row.shape).or_default() += 1;
    }
    println!("retail compressed shapes: {shapes:?}");
    assert_eq!(
        shapes.len(),
        7,
        "the seven distinct retail compressed shapes: IMA mono 11025/256, and MS ADPCM at mono \
         11025/256, 22000/512, 22050/512, 44100/1024 and stereo 9710/512 and 22050/1024"
    );
}

// --- Evidence harness ---------------------------------------------------------

/// Evidence-report harness for task #444 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T444
///    cargo test --workspace --locked -- accept_t444_ --include-ignored \
///      2>&1 | tee private/evidence/T444/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T444 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t444_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test zbd -- evidence_report_t444 --ignored
///    ```
///    `CS_FFMPEG` may name an ffmpeg binary. When it does, the harness also
///    decodes one member of every distinct retail shape with production code and
///    with that independent implementation of the same two formats, and records
///    the comparison. Everything it extracts stays in the private directory.
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T444/acceptance.json \
///      --artifact-root private/evidence/T444 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T444.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t444_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = game_dir();

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log, "accept_t444_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t444_` tests were recorded in {}",
        log_path.display()
    );
    let retail = "accept_t444_retail_every_compressed_sound_member_decodes_to_its_declared_counts";
    let status = suite
        .assertions
        .iter()
        .find(|(name, _)| short_name(name) == retail)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
    assert_eq!(status, "pass", "{retail} must pass");

    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery reads the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();
    let hashes: BTreeMap<String, String> = found
        .manifest
        .files
        .iter()
        .map(|record| {
            (
                record.relative_spelling.as_str().to_owned(),
                record.sha256.to_hex(),
            )
        })
        .collect();

    // Per-archive decode facts: counts and shapes, no original bytes.
    let rows = retail_adpcm();
    let mut by_archive: BTreeMap<String, Vec<&RetailMember>> = BTreeMap::new();
    for row in &rows {
        by_archive
            .entry(row.spelling.clone())
            .or_default()
            .push(row);
    }
    let archive_rows: Vec<String> = by_archive
        .iter()
        .map(|(spelling, members)| {
            let sha = hashes
                .get(spelling)
                .map_or_else(|| "null".to_owned(), |hash| jstr(hash));
            let blocks: u64 = members.iter().map(|row| row.blocks).sum();
            let samples: u64 = members.iter().map(|row| row.samples).sum();
            let bytes: u64 = members.iter().map(|row| row.bytes).sum();
            let saturated: u64 = members.iter().map(|row| row.saturated).sum();
            let mut shapes: BTreeMap<Shape, usize> = BTreeMap::new();
            for row in members {
                *shapes.entry(row.shape).or_default() += 1;
            }
            let shape_rows: Vec<String> = shapes
                .iter()
                .map(|(shape, count)| {
                    format!("{{\"shape\": {}, \"members\": {count}}}", shape.json())
                })
                .collect();
            format!(
                "{{\"spelling\": {}, \"sha256\": {sha}, \"compressed_members\": {}, \"blocks\": \
                 {blocks}, \"decoded_samples\": {samples}, \"decoded_bytes\": {bytes}, \
                 \"saturated_samples\": {saturated}, \"shapes\": [{}]}}",
                jstr(spelling),
                members.len(),
                shape_rows.join(", ")
            )
        })
        .collect();
    let decode_path = evidence_dir.join("zbd-adpcm-decode.json");
    fs::write(
        &decode_path,
        format!(
            "{{\n \"task_id\": \"T444\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"compressed_members\": {},\n \"archives\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            rows.len(),
            archive_rows.join(",\n  ")
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", decode_path.display()));

    let reference = reference_comparison(&evidence_dir, &rows);
    let reference_json = match &reference {
        Some(comparison) => format!(
            "{{\"implementation\": \"ffmpeg\", \"version\": {}, \"shapes_compared\": {}, \
             \"samples_compared\": {}, \"mismatching_samples\": 0}}",
            jstr(&comparison.version),
            comparison.shapes,
            comparison.samples
        ),
        None => "null".to_owned(),
    };
    let unknowns = [
        Unknown {
            item: "multi-channel IMA ADPCM, and MS ADPCM beyond two channels",
            status: "unknown",
            why: "no retail member declares them and the layouts documented for wider blocks \
                   differ, so they are refused rather than read",
            affected: "0 retail members",
        },
        Unknown {
            item: "what the original executable does with the decoded samples: pitch, volume, \
                   spatialisation, and whether it loops a sound",
            status: "unknown",
            why: "decided outside these bytes; no retail WAVE header declares a loop region",
            affected: "every sound member; feature F41",
        },
        Unknown {
            item: "a trailing short block",
            status: "not observed in retail",
            why: "every retail `data` payload is a whole number of `nBlockAlign` blocks, so the \
                   sample count of a short final block follows the documented geometry and is \
                   covered by synthetic tests only",
            affected: "0 retail members",
        },
        Unknown {
            item: "whether the `cs_assets` consumer (which still calls stage F06-C's \
                   `SampleFormat::from_header`) reports these members as decoded",
            status: "known gap, filed as a follow-up task",
            why: "switching that consumer changes its F06-C `SoundReadiness::UnsupportedFormat` \
                   contract and is outside this task's owner paths",
            affected: "every sound member at runtime",
        },
    ];
    let unknown_json: Vec<String> = unknowns
        .iter()
        .map(|unknown| {
            format!(
                "{{\"item\": {}, \"status\": {}, \"why\": {}, \"affected_content\": {}}}",
                jstr(unknown.item),
                jstr(unknown.status),
                jstr(unknown.why),
                jstr(unknown.affected)
            )
        })
        .collect();

    let artifacts = [artifact(&log_path, "log"), artifact(&decode_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T444\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"independent_reference\": {reference_json},\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&command_output("rustc", &["--version"])),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.passed + suite.failed + suite.ignored,
        suite.passed + suite.failed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(short_name(name))
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        unknown_json.join(", "),
        jstr(
            "implementer: bunny-alpha-1/bunny-alpha-1 (Rally #444, implement claim of \
             2026-10-02T14:16:39Z); reviewer: none yet. The implementer did not review this \
             work, and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, and the \
             production index reader, sound reader, WAVE header reader and block decoder over \
             every compressed member of both retail sound archives \
             (zbd-adpcm-decode.json); when CS_FFMPEG names an ffmpeg binary it also compares \
             production decodes with that independent implementation of the same two documented \
             formats sample for sample; validated with tools/validate_evidence.py --require-pass. \
             The decode is a checked format claim, not a claim about how the original executable \
             played the sound, and the unknowns above name what these files do not decide"
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must not validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// One entry of the evidence report's unresolved-issue list.
struct Unknown {
    item: &'static str,
    status: &'static str,
    why: &'static str,
    affected: &'static str,
}

/// The result of comparing production decodes with an independent implementation.
struct Comparison {
    version: String,
    shapes: usize,
    samples: u64,
}

/// Decodes one member of every distinct retail shape with production code and,
/// when `CS_FFMPEG` names an ffmpeg binary, with that independent implementation
/// of the same two documented formats, and compares the two sample sequences.
///
/// A disagreement panics: both sides implement the same published layouts, so a
/// difference is a finding, not a tolerance to absorb. Everything written here
/// stays in the private evidence directory.
fn reference_comparison(evidence_dir: &Path, rows: &[RetailMember]) -> Option<Comparison> {
    let ffmpeg = std::env::var("CS_FFMPEG").ok()?;
    let mut shapes: BTreeMap<Shape, &RetailMember> = BTreeMap::new();
    for row in rows {
        shapes.entry(row.shape).or_insert(row);
    }
    let mut compared_shapes = 0usize;
    let mut compared_samples = 0u64;
    for (shape, row) in shapes {
        let declared = SampleFormat::from_member(&row.content).expect("the member declares a plan");
        let mut context = ParseContext::with_defaults(format!("retail/{}", row.name));
        let mine = decode_sound_sample(&mut context, &row.content, &declared).expect("decodes");
        let stem = format!(
            "reference-{tag:04x}-{channels}ch-{align}",
            tag = shape.tag,
            channels = shape.channels,
            align = shape.block_align
        );
        let extracted = evidence_dir.join(format!("{stem}.wav"));
        fs::write(&extracted, &row.content)
            .unwrap_or_else(|error| panic!("write {}: {error}", extracted.display()));
        let output = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                &extracted.to_string_lossy(),
                "-f",
                "s16le",
                "-",
            ])
            .output()
            .unwrap_or_else(|error| panic!("run {ffmpeg}: {error}"));
        assert!(
            output.status.success(),
            "{ffmpeg} failed on {}: {}",
            row.name,
            String::from_utf8_lossy(&output.stderr)
        );
        let raw = evidence_dir.join(format!("{stem}.s16le"));
        fs::write(&raw, &output.stdout)
            .unwrap_or_else(|error| panic!("write {}: {error}", raw.display()));
        let theirs: Vec<i32> = output
            .stdout
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i32::from(i16::from_le_bytes(*pair)))
            .collect();
        assert_eq!(
            mine.samples(),
            theirs.as_slice(),
            "{} ({stem}): the production decode differs from {ffmpeg}",
            row.name
        );
        compared_shapes += 1;
        compared_samples += mine.sample_count();
    }
    let version = command_output(&ffmpeg, &["-version"])
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    Some(Comparison {
        version,
        shapes: compared_shapes,
        samples: compared_samples,
    })
}
