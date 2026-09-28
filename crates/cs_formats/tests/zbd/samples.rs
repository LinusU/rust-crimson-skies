//! Acceptance stage F06-C: decoding a member's `data` payload under the
//! format its **own** RIFF/WAVE header declares
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
//! section `### F06-C`, AC03 — "decode a short sound sample and compare
//! byte/sample count with its declared format").
//!
//! Every byte here is authored in this file: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access.
//!
//! The fixtures are complete RIFF/WAVE members written by
//! [`wave_member`] from the fields the Microsoft/IBM *Multimedia Programming
//! Interface and Data Specifications 1.0* documents (task #344's source), so
//! the production `read_wave_header` parses them exactly as it parses a real
//! member and the tests never hand the decoder a format it did not read from
//! the member's own bytes.

use cs_formats::zbd::{
    PcmLayout, SampleError, SampleFormat, SampleFormatError, WAVE_FORMAT_IMA_ADPCM,
    WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM, decode_payload, decode_sound_sample, read_wave_header,
};
use cs_formats::{ParseContext, ParseErrorKind};

/// Provenance label carried by every result and error these tests assert on.
const CONTAINER: &str = "synthetic/f06_c_wave.zbd";

/// The sample values the fixtures store, in declaration order.
const RAMP: [i16; 8] = [0, 1, 2, 3, -1, -2, -3, -4];

/// A fixture member's `fmt ` fields, all as the Microsoft/IBM spec states
/// them.
struct Fmt {
    tag: u16,
    channels: u16,
    rate_hz: u32,
    bits_per_sample: u16,
}

impl Fmt {
    /// The one 8-bit mono PCM shape task #344 measured in retail.
    const fn pcm8() -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels: 1,
            rate_hz: 11_025,
            bits_per_sample: 8,
        }
    }

    /// 16-bit mono PCM: the conventional CD-rate shape.
    const fn pcm16() -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels: 1,
            rate_hz: 22_050,
            bits_per_sample: 16,
        }
    }

    /// 16-bit stereo PCM, so a frame is four bytes of two interleaved
    /// channels.
    const fn pcm16_stereo() -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels: 2,
            rate_hz: 44_100,
            bits_per_sample: 16,
        }
    }

    /// 32-bit mono PCM.
    const fn pcm32() -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels: 1,
            rate_hz: 48_000,
            bits_per_sample: 32,
        }
    }

    /// The IMA ADPCM shape task #344 measured most often in retail.
    const fn ima_adpcm() -> Self {
        Self {
            tag: WAVE_FORMAT_IMA_ADPCM,
            channels: 1,
            rate_hz: 11_025,
            bits_per_sample: 4,
        }
    }

    /// The Microsoft ADPCM shape task #344 measured in retail.
    const fn ms_adpcm() -> Self {
        Self {
            tag: WAVE_FORMAT_MS_ADPCM,
            channels: 1,
            rate_hz: 22_050,
            bits_per_sample: 4,
        }
    }

    /// A PCM tag at a width this stage does not decode.
    const fn pcm24() -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels: 1,
            rate_hz: 44_100,
            bits_per_sample: 24,
        }
    }

    /// `nBlockAlign` the fields imply: `nChannels * bits / 8`.
    const fn block_align(&self) -> u16 {
        self.channels * (self.bits_per_sample / 8)
    }

    /// A format-specific `fmt ` tail, as ADPCM carries its coefficient table.
    fn fmt_tail(&self) -> Vec<u8> {
        if self.tag == WAVE_FORMAT_PCM {
            Vec::new()
        } else {
            // Two extra u16 words: samples-per-block and block-count, the
            // first two format-specific fields the RIFF spec defines for
            // ADPCM. Their values do not matter to this stage.
            let mut tail = Vec::with_capacity(4);
            tail.extend_from_slice(&2u16.to_le_bytes());
            tail.extend_from_slice(&1u16.to_le_bytes());
            tail
        }
    }
}

/// Writes a four-byte chunk id.
fn id(bytes: &[u8; 4]) -> [u8; 4] {
    *bytes
}

/// Assembles a complete RIFF/WAVE member around `fmt` and a `data` payload.
///
/// The layout is the one task #344's source documents: `RIFF`, a u32 size
/// counting every byte after the size word, the `WAVE` form type, then chunks
/// of id, u32 size and payload, with a pad byte after an odd payload.
fn wave_member(fmt: &Fmt, data: &[u8]) -> Vec<u8> {
    let fmt_payload_len = 16 + fmt.fmt_tail().len();
    let mut fmt_payload = Vec::with_capacity(fmt_payload_len);
    fmt_payload.extend_from_slice(&fmt.tag.to_le_bytes());
    fmt_payload.extend_from_slice(&fmt.channels.to_le_bytes());
    fmt_payload.extend_from_slice(&fmt.rate_hz.to_le_bytes());
    // `nAvgBytesPerSec` is not read by the header reader, so a fixture value
    // is fine; it is the conventional rate * block align.
    fmt_payload.extend_from_slice(&(fmt.rate_hz * u32::from(fmt.block_align())).to_le_bytes());
    fmt_payload.extend_from_slice(&fmt.block_align().to_le_bytes());
    fmt_payload.extend_from_slice(&fmt.bits_per_sample.to_le_bytes());
    fmt_payload.extend_from_slice(&fmt.fmt_tail());

    let mut chunks = Vec::new();
    chunks.extend_from_slice(&id(b"fmt "));
    chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&fmt_payload);
    if fmt_payload.len() % 2 == 1 {
        chunks.push(0);
    }
    chunks.extend_from_slice(&id(b"data"));
    chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunks.extend_from_slice(data);
    if data.len() % 2 == 1 {
        chunks.push(0);
    }

    let mut member = Vec::with_capacity(12 + chunks.len());
    member.extend_from_slice(b"RIFF");
    member.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
    member.extend_from_slice(b"WAVE");
    member.extend_from_slice(&chunks);
    member
}

/// The `data` payload of an eight-sample 16-bit mono fixture.
fn ramp16() -> Vec<u8> {
    let mut data = Vec::with_capacity(RAMP.len() * 2);
    for sample in RAMP {
        data.extend_from_slice(&sample.to_le_bytes());
    }
    data
}

/// Reads a member's header and builds the decode plan it declares.
fn plan(member: &[u8]) -> SampleFormat {
    let header = read_wave_header(member).expect("the fixture member is a readable RIFF/WAVE file");
    SampleFormat::from_header(&header).expect("the fixture declares a decodable format")
}

// --- AC03: decode a short sound sample and compare with its declared format

#[test]
fn accept_f06_c_a_short_sound_sample_matches_its_declared_byte_and_sample_count() {
    // The stage's minimum scenario, end to end: a member's own WAVE header
    // declares its format, its `data` payload is decoded under exactly that
    // declaration, and the byte and sample counts are compared with what the
    // declaration implies.
    let data = ramp16();
    let member = wave_member(&Fmt::pcm16(), &data);
    let declared = plan(&member);

    // The plan is the member's own header, read by the production reader.
    assert_eq!(declared.layout(), PcmLayout::Signed16Le);
    assert_eq!(declared.channels(), 1);
    assert_eq!(declared.rate_hz(), 22_050);
    assert_eq!(declared.block_align(), 2);
    assert_eq!(declared.frame_bytes(), 2);
    assert_eq!(declared.samples_per_frame(), 1);
    assert_eq!(declared.data_span().length, data.len() as u64);

    let mut context = ParseContext::with_defaults(CONTAINER);
    let decoded =
        decode_sound_sample(&mut context, &member, &declared).expect("whole frames decode");

    // The comparison the acceptance case asks for: the byte count the decode
    // accounted for is the payload the member declares, and the sample count
    // is the one the declared format implies.
    assert_eq!(decoded.byte_len(), data.len() as u64);
    assert_eq!(decoded.frames(), 8);
    assert_eq!(decoded.samples_per_frame(), 1);
    assert_eq!(decoded.sample_count(), 8);
    assert_eq!(
        decoded.byte_len(),
        decoded.sample_count() * declared.layout().bytes_per_sample(),
        "byte count = sample count * bytes per sample"
    );
    assert_eq!(
        decoded.byte_len(),
        decoded.frames() * declared.frame_bytes(),
        "byte count = frames * declared frame size"
    );
    assert_eq!(decoded.format(), &declared);

    // The values are the stored samples, little-endian, in declaration order.
    assert_eq!(decoded.samples().len(), RAMP.len());
    for (index, expected) in RAMP.iter().enumerate() {
        assert_eq!(
            decoded.samples()[index],
            i32::from(*expected),
            "sample {index}"
        );
    }
    // A mono frame is one declared channel, so it holds one value.
    assert_eq!(decoded.frame(0), Some(&[0][..]));
    assert_eq!(decoded.frame(4), Some(&[-1][..]));
    assert_eq!(
        decoded.frame(8),
        None,
        "a frame past the end is not a frame"
    );
}

#[test]
fn accept_f06_c_the_declared_format_decides_what_a_frame_is() {
    // One 32-byte body, read three different ways: purely from what each
    // member's own header declares. Nothing here is a preference: every
    // number below is that member's `nBlockAlign` and `nChannels`.
    let data = {
        let mut bytes = Vec::with_capacity(32);
        for sample in [0i16, 1, 2, 3, 4, 5, 6, 7, 8, -1, -2, -3, -4, -5, -6, -7] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    };
    assert_eq!(data.len(), 32);

    let mut context = ParseContext::with_defaults(CONTAINER);

    // 8-bit mono: one byte per frame, so thirty-two frames.
    let eight = wave_member(&Fmt::pcm8(), &data);
    let declared = plan(&eight);
    assert_eq!(declared.frame_bytes(), 1);
    let decoded = decode_sound_sample(&mut context, &eight, &declared).expect("32 whole frames");
    assert_eq!(decoded.frames(), 32);
    assert_eq!(decoded.sample_count(), 32);
    assert_eq!(decoded.byte_len(), 32);
    // The 8-bit decode reads the stored bytes as they are: the little-endian
    // pairs of the same body read back one at a time.
    assert_eq!(decoded.samples()[0], 0);
    assert_eq!(decoded.samples()[1], 0);
    assert_eq!(decoded.samples()[2], 1);
    assert_eq!(decoded.samples()[3], 0);

    // 16-bit stereo: four bytes per frame of two channels, so eight frames
    // and sixteen values.
    let stereo = wave_member(&Fmt::pcm16_stereo(), &data);
    let declared = plan(&stereo);
    assert_eq!(declared.frame_bytes(), 4);
    assert_eq!(declared.samples_per_frame(), 2);
    let decoded = decode_sound_sample(&mut context, &stereo, &declared).expect("8 whole frames");
    assert_eq!(decoded.frames(), 8);
    assert_eq!(decoded.sample_count(), 16);
    assert_eq!(decoded.byte_len(), 32);
    // A frame is two interleaved channels, so it yields two values.
    assert_eq!(decoded.frame(0), Some(&[0, 1][..]));
    assert_eq!(decoded.frame(1), Some(&[2, 3][..]));
    assert_eq!(decoded.frame(8), None);

    // 32-bit mono: four bytes per frame, so eight frames.
    let wide = wave_member(&Fmt::pcm32(), &data);
    let declared = plan(&wide);
    assert_eq!(declared.layout(), PcmLayout::Signed32Le);
    assert_eq!(declared.frame_bytes(), 4);
    let decoded = decode_sound_sample(&mut context, &wide, &declared).expect("8 whole frames");
    assert_eq!(decoded.frames(), 8);
    assert_eq!(decoded.sample_count(), 8);
    assert_eq!(decoded.byte_len(), 32);
    // A 32-bit frame reads four stored bytes as one little-endian value, so
    // the same body reads as different numbers at a different width: the
    // decode follows the declaration, it does not reinterpret the samples.
    assert_eq!(decoded.frame(0), Some(&[0x0001_0000][..]));
    assert_eq!(decoded.frame(1), Some(&[0x0003_0002][..]));
    assert_eq!(decoded.frame(4), Some(&[-65_528][..]));
    assert_eq!(decoded.frame(8), None);
}

// --- Failure cases the acceptance scenario implies

#[test]
fn accept_f06_c_a_data_payload_that_is_not_whole_frames_is_refused_not_truncated() {
    // Seventeen bytes under a two-byte mono frame: one trailing byte. The
    // decode must refuse rather than round down to eight frames, because a
    // truncated sample would silently drop the last one.
    let mut data = ramp16();
    data.push(0xAB);
    let member = wave_member(&Fmt::pcm16(), &data);
    let declared = plan(&member);
    assert_eq!(declared.data_span().length, 17);

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_sound_sample(&mut context, &member, &declared)
        .expect_err("a partial trailing frame is not a sample");
    assert_eq!(error.code(), "partial_frame");
    assert_eq!(
        error,
        SampleError::PartialFrame {
            bytes: 17,
            frame_bytes: 2,
            remainder: 1,
        }
    );
    assert_eq!(error.offset(), 17);
    assert!(error.to_string().contains("17"), "{error}");
    assert!(error.to_string().contains("2-byte"), "{error}");

    // A refusal charges nothing, and the same context still decodes the whole
    // frames of a different member.
    assert_eq!(context.allocation().used(), 0);
    let whole = wave_member(&Fmt::pcm16(), &ramp16());
    let declared = plan(&whole);
    let decoded = decode_sound_sample(&mut context, &whole, &declared).expect("whole frames");
    assert_eq!(decoded.frames(), 8);
    assert_eq!(decoded.byte_len(), 16);
}

#[test]
fn accept_f06_c_a_member_declaring_a_compressed_format_is_refused_with_its_own_tag() {
    // Task #344 measured that nearly every retail member is IMA or MS ADPCM.
    // This stage does not decode them: each is refused carrying the tag the
    // member itself declares, so the row is visible and honest rather than
    // passed through as if it were PCM.
    for (fmt, tag, name) in [
        (Fmt::ima_adpcm(), WAVE_FORMAT_IMA_ADPCM, Some("ima_adpcm")),
        (Fmt::ms_adpcm(), WAVE_FORMAT_MS_ADPCM, Some("ms_adpcm")),
    ] {
        let member = wave_member(&fmt, &[0u8; 256]);
        let header = read_wave_header(&member).expect("the ADPCM fixture is readable");
        assert_eq!(header.format_tag(), tag);
        assert_eq!(header.format_name(), name);

        let error = SampleFormat::from_header(&header)
            .expect_err("this stage decodes uncompressed PCM only");
        assert_eq!(error.code(), "unsupported_format");
        assert_eq!(
            error,
            SampleFormatError::UnsupportedFormat { tag, name },
            "the refusal carries the member's own declared tag and name"
        );
        let text = error.to_string();
        assert!(text.contains(&format!("0x{tag:04X}")), "{text}");
        if let Some(name) = name {
            assert!(text.contains(name), "{text}");
        }
    }
}

#[test]
fn accept_f06_c_a_declaration_this_stage_cannot_decode_is_refused_with_its_own_value() {
    // A PCM member at a width this stage does not decode, and a member whose
    // header does not read at all. Each refusal names the member's own value
    // or the header reader's own reason.
    let wide = wave_member(&Fmt::pcm24(), &[0u8; 24]);
    let header = read_wave_header(&wide).expect("the 24-bit fixture is readable");
    assert_eq!(header.bits_per_sample(), 24);
    let error =
        SampleFormat::from_header(&header).expect_err("24-bit PCM is not decoded by this stage");
    assert_eq!(error.code(), "unsupported_width");
    assert_eq!(
        error,
        SampleFormatError::UnsupportedWidth {
            bits_per_sample: 24
        }
    );
    assert!(error.to_string().contains("24"), "{error}");

    // A member that is not RIFF at all: the header reader refuses it, and the
    // decode never starts.
    let not_riff = b"NOTRIFFxx not a wave file at all".to_vec();
    let wave_error = read_wave_header(&not_riff).expect_err("the body is not a RIFF file");
    assert_eq!(wave_error.code(), "not_riff");
    let error = SampleError::from_wave_header(wave_error);
    assert_eq!(error.code(), "unreadable_header");
    assert_eq!(error.reason(), Some(wave_error.reason()));
    assert_eq!(wave_error.code(), "not_riff");
    assert!(error.to_string().contains("RIFF"), "{error}");
}

#[test]
fn accept_f06_c_a_header_that_contradicts_itself_is_refused() {
    // `nBlockAlign` that does not match `nChannels * bytes_per_sample` is a
    // self-contradictory declaration, not something to guess past: the frame
    // arithmetic cannot be done, so the plan refuses and names both numbers.
    let mut member = wave_member(&Fmt::pcm16(), &ramp16());
    // `nBlockAlign` sits at member offset 32: RIFF(4) + size(4) + WAVE(4) +
    // "fmt "(4) + size(4) + tag(2) + channels(2) + rate(4) + avg(4).
    let align_offset = 32;
    member[align_offset..align_offset + 2].copy_from_slice(&7u16.to_le_bytes());
    let header = read_wave_header(&member).expect("the header itself still reads");
    assert_eq!(header.block_align(), 7);
    assert_eq!(header.channels(), 1);

    let error = SampleFormat::from_header(&header).expect_err("a contradictory align is refused");
    assert_eq!(error.code(), "block_align_mismatch");
    assert_eq!(
        error,
        SampleFormatError::BlockAlignMismatch {
            declared: 7,
            implied: 2,
        }
    );
    let text = error.to_string();
    assert!(text.contains('7'), "{text}");
    assert!(text.contains('2'), "{text}");
}

#[test]
fn accept_f06_c_the_decode_is_bounded_by_the_parse_allocation_budget() {
    // The sample buffer is booked before it exists, so a starved context
    // refuses a short sample without allocating, the refusal is scoped
    // `zbd.sample.sample.values`, and the retry on a funded context decodes the
    // very same member.
    let member = wave_member(&Fmt::pcm16(), &ramp16());
    let declared = plan(&member);
    let exact = 8 * SAMPLE_VALUE_BYTES_FOR_TEST;

    // One byte short of the exact charge: refused, nothing charged.
    let mut starved = ParseContext::new(CONTAINER, exact - 1, 32);
    let error = decode_sound_sample(&mut starved, &member, &declared)
        .expect_err("a budget one byte short of the exact charge is refused");
    assert_eq!(error.code(), "allocation_budget_exceeded");
    let SampleError::Parse(parse) = &error else {
        panic!("expected a parse failure, got {error:?}")
    };
    assert_eq!(parse.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(parse.container, CONTAINER);
    assert_eq!(parse.field, "zbd.sample.sample.values");
    assert_eq!(
        parse.expected,
        format!(
            "{} of {} allocation-budget bytes available",
            exact - 1,
            exact - 1
        )
    );
    assert_eq!(parse.observed, format!("{exact} bytes requested"));
    // The refused attempt is rolled back and nothing is allocated.
    assert_eq!(starved.allocation().used(), 0);
    assert_eq!(starved.allocation().limit(), exact - 1);
    assert_eq!(starved.recursion().depth(), 0);

    // The exact charge fits, and the same member decodes.
    let mut funded = ParseContext::new(CONTAINER, exact, 32);
    let decoded =
        decode_sound_sample(&mut funded, &member, &declared).expect("the exact charge fits");
    assert_eq!(decoded.sample_count(), 8);
    assert_eq!(decoded.byte_len(), 16);
    assert_eq!(funded.allocation().used(), exact);
}

/// Bytes one decoded value occupies, the charge the decode books.
const SAMPLE_VALUE_BYTES_FOR_TEST: u64 = size_of::<i32>() as u64;

#[test]
fn accept_f06_c_an_empty_payload_is_zero_frames_not_an_error() {
    // A member with an empty `data` chunk simply holds no samples, which is
    // a whole number of zero frames.
    let member = wave_member(&Fmt::pcm16(), &[]);
    let declared = plan(&member);
    assert_eq!(declared.data_span().length, 0);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let decoded = decode_sound_sample(&mut context, &member, &declared)
        .expect("an empty payload is zero whole frames");
    assert_eq!(decoded.frames(), 0);
    assert_eq!(decoded.sample_count(), 0);
    assert_eq!(decoded.byte_len(), 0);
    assert!(decoded.samples().is_empty());
    assert_eq!(decoded.frame(0), None);
}

#[test]
fn accept_f06_c_the_data_payload_can_be_decoded_on_its_own() {
    // `decode_payload` is the same decode over a payload the caller already
    // holds (a private research export, or a member extracted elsewhere), so
    // the byte/sample accounting is identical either way.
    let data = ramp16();
    let member = wave_member(&Fmt::pcm16(), &data);
    let declared = plan(&member);

    let mut from_member = ParseContext::with_defaults(CONTAINER);
    let whole = decode_sound_sample(&mut from_member, &member, &declared).expect("decodes");
    let mut from_payload = ParseContext::with_defaults(CONTAINER);
    let payload = decode_payload(&mut from_payload, &data, &declared).expect("decodes");
    assert_eq!(whole, payload);
    assert_eq!(
        from_member.allocation().used(),
        from_payload.allocation().used()
    );
}
