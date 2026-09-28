//! Acceptance stage F07-A: raw INTERP records and the golden synthetic
//! fixture (`specs/F07-interp-loading-script-container.md`, section
//! `### F07-A`), and stage F07-B: the validating lossless token decoder
//! (`### F07-B`), whose tests are the `accept_f07_b_*` ones at the bottom.
//!
//! Every byte in this file is authored here, except
//! `fixtures/synthetic/synthetic.interp` and `bad-version.interp`, whose
//! bytes are authored by `tools/make_synthetic_fixtures.py` and whose
//! expected values are asserted independently of this crate's reader
//! (`fixtures/synthetic/expected.json`). No original game data, no
//! `CS_GAME_DIR` access.

use cs_formats::{
    DecodedInterp, INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, InterpError, InterpFile, InterpToken,
    LINE_HEADER_BYTES, NAME_FIELD_BYTES, ParseContext, ParseErrorKind, RawArgument, decode_interp,
    read_interp,
};

const SYNTHETIC: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic.interp");
const BAD_VERSION: &[u8] = include_bytes!("../../../fixtures/synthetic/bad-version.interp");

/// Provenance label carried by every error the authored cases assert on.
const CONTAINER: &str = "synthetic/f07_a.interp";

/// One authored script: name, timestamp and lines of raw argument data with
/// their declared argument counts.
struct Script<'a> {
    name: &'a [u8],
    timestamp: u32,
    lines: Vec<(u32, &'a [u8])>,
}

/// Builds a container: header, index, then the scripts back to back, each
/// ended by a zero `size` word. Returns the bytes and every script offset.
fn container(scripts: &[Script<'_>]) -> (Vec<u8>, Vec<u32>) {
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    let body_start = INTERP_HEADER_BYTES + scripts.len() * INDEX_ENTRY_BYTES;
    for script in scripts {
        offsets.push((body_start + body.len()) as u32);
        for (count, data) in &script.lines {
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&count.to_le_bytes());
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut bytes = Vec::new();
    for word in [0x0897_1119u32, 7, scripts.len() as u32] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for (script, offset) in scripts.iter().zip(&offsets) {
        let mut name = [0u8; NAME_FIELD_BYTES];
        name[..script.name.len()].copy_from_slice(script.name);
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&script.timestamp.to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    (bytes, offsets)
}

fn read(bytes: &[u8]) -> Result<InterpFile<'_>, InterpError> {
    read_interp(&mut ParseContext::with_defaults(CONTAINER), bytes)
}

fn arguments<'a>(file: &InterpFile<'a>, script: usize, line: usize) -> Vec<RawArgument<'a>> {
    file.scripts()[script].lines[line].raw_arguments().collect()
}

fn parse_error(error: InterpError) -> cs_formats::ParseError {
    match error {
        InterpError::Parse(error) => error,
        other => panic!("expected a structural parse error, got {other:?}"),
    }
}

/// AC01: the generated synthetic fixture parses and its second line keeps
/// its two arguments exactly — bytes, boundaries and absolute offsets.
#[test]
fn accept_f07_a_shared_fixture_preserves_two_arguments_exactly() {
    let mut context = ParseContext::with_defaults("fixtures/synthetic/synthetic.interp");
    let file = read_interp(&mut context, SYNTHETIC).expect("the synthetic INTERP must parse");

    // expected.json: signature 144118041, version 7, names ["synthetic_probe"].
    let header = file.header();
    assert_eq!(header.signature, 144_118_041);
    assert_eq!(header.version, 7);
    assert_eq!(header.script_count, 1);
    assert_eq!(file.scripts().len(), 1);
    assert_eq!(file.index_end(), 140);

    let script = &file.scripts()[0];
    assert_eq!(script.entry.index, 0);
    assert_eq!(script.entry.entry_offset, 12);
    assert_eq!(script.entry.name_bytes(), b"synthetic_probe");
    assert_eq!(script.entry.raw_timestamp, 0);
    assert_eq!(script.entry.script_offset, 140);
    assert_eq!(script.lines.len(), 2);

    // Line 0 at 140: size 21, count 3, "SYNTHETIC\0alpha\0beta\0".
    let first = &script.lines[0];
    assert_eq!(
        (first.offset, first.size, first.argument_count),
        (140, 21, 3)
    );
    assert_eq!(first.data, b"SYNTHETIC\0alpha\0beta\0");
    let tokens: Vec<&[u8]> = arguments(&file, 0, 0).iter().map(|a| a.bytes).collect();
    assert_eq!(tokens, [&b"SYNTHETIC"[..], b"alpha", b"beta"]);

    // Line 1 at 140 + 8 + 21 = 169: the two-argument line.
    let second = &script.lines[1];
    assert_eq!(
        (second.offset, second.size, second.argument_count),
        (169, 9, 2)
    );
    assert_eq!(second.data, b"VALUE\x0042\0");
    assert_eq!(
        arguments(&file, 0, 1),
        [
            RawArgument {
                offset: 177,
                bytes: b"VALUE",
                terminated: true
            },
            RawArgument {
                offset: 183,
                bytes: b"42",
                terminated: true
            },
        ]
    );
    assert_eq!(&SYNTHETIC[177..182], b"VALUE");
    assert_eq!(&SYNTHETIC[183..185], b"42");

    // The zero terminator is the last word of the file.
    assert_eq!(script.terminator_offset, 186);
    assert_eq!(script.end(), SYNTHETIC.len() as u64);
}

/// Arguments that hold spaces, are empty or are left without a final NUL
/// keep their boundaries: a joined string would make `"a b","c"` and
/// `"a","b c"` indistinguishable.
#[test]
fn accept_f07_a_argument_boundaries_survive_spaces_and_empty_arguments() {
    let (bytes, offsets) = container(&[Script {
        name: b"boundaries",
        timestamp: 0,
        lines: vec![(2, b"a b\0c\0"), (2, b"a\0b c\0"), (3, b"\0x\0tail")],
    }]);
    let file = read(&bytes).expect("authored container parses");
    let data_start = u64::from(offsets[0]) + LINE_HEADER_BYTES as u64;

    let first: Vec<&[u8]> = arguments(&file, 0, 0).iter().map(|a| a.bytes).collect();
    let second: Vec<&[u8]> = arguments(&file, 0, 1).iter().map(|a| a.bytes).collect();
    assert_eq!(first, [&b"a b"[..], b"c"]);
    assert_eq!(second, [&b"a"[..], b"b c"]);
    assert_ne!(first, second);

    let third = arguments(&file, 0, 2);
    let line = &file.scripts()[0].lines[2];
    let base = line.data_offset();
    assert_eq!(base, data_start + 6 + 8 + 6 + 8);
    assert_eq!(
        third,
        [
            RawArgument {
                offset: base,
                bytes: b"",
                terminated: true
            },
            RawArgument {
                offset: base + 1,
                bytes: b"x",
                terminated: true
            },
            RawArgument {
                offset: base + 3,
                bytes: b"tail",
                terminated: false
            },
        ]
    );
    // Lossless: reassembling the arguments reproduces the stored data.
    let mut rebuilt = Vec::new();
    for argument in &third {
        rebuilt.extend_from_slice(argument.bytes);
        if argument.terminated {
            rebuilt.push(0);
        }
    }
    assert_eq!(rebuilt, line.data);
    assert_eq!(line.argument_count, 3, "the count word is kept verbatim");
}

/// Two scripts with equal names keep distinct origins (index position,
/// entry offset, script offset); the timestamp stays raw metadata.
#[test]
fn accept_f07_a_equal_names_keep_distinct_origins() {
    let (bytes, offsets) = container(&[
        Script {
            name: b"twin",
            timestamp: 0x1234_5678,
            lines: vec![(1, b"ONE\0")],
        },
        Script {
            name: b"twin",
            timestamp: 0x1234_5678,
            lines: vec![(1, b"TWO\0")],
        },
    ]);
    let file = read(&bytes).expect("authored container parses");
    let [a, b] = file.scripts() else {
        panic!("two scripts expected")
    };
    assert_eq!(a.entry.name_bytes(), b.entry.name_bytes());
    assert_eq!(a.entry.raw_timestamp, 0x1234_5678);
    assert_eq!((a.entry.index, b.entry.index), (0, 1));
    assert_eq!((a.entry.entry_offset, b.entry.entry_offset), (12, 140));
    assert_eq!(
        (a.entry.script_offset, b.entry.script_offset),
        (offsets[0], offsets[1])
    );
    assert_eq!(a.lines[0].data, b"ONE\0");
    assert_eq!(b.lines[0].data, b"TWO\0");
    assert_eq!(a.end(), u64::from(offsets[1]));
}

/// The name field is kept whole: padding after the first NUL is not
/// discarded by the raw record.
#[test]
fn accept_f07_a_name_field_is_kept_verbatim() {
    let (mut bytes, _) = container(&[Script {
        name: b"padded",
        timestamp: 7,
        lines: vec![(1, b"X\0")],
    }]);
    bytes[INTERP_HEADER_BYTES + 100] = 0xAB;
    let file = read(&bytes).expect("authored container parses");
    let entry = file.scripts()[0].entry;
    assert_eq!(entry.name_bytes(), b"padded");
    assert_eq!(entry.name_field.len(), NAME_FIELD_BYTES);
    assert_eq!(entry.name_field[100], 0xAB);
}

/// The generated `bad-version.interp` (version 999) and a wrong signature
/// are refused with the offending word and its offset.
#[test]
fn accept_f07_a_undocumented_header_is_rejected() {
    let error = read(BAD_VERSION).expect_err("version 999 is not documented");
    assert_eq!(
        error,
        InterpError::Version {
            container: CONTAINER.to_owned(),
            offset: 4,
            observed: 999
        }
    );
    assert_eq!(error.code(), "version");

    let mut wrong = SYNTHETIC.to_vec();
    wrong[0] ^= 0xFF;
    let error = read(&wrong).expect_err("wrong signature");
    assert_eq!(error.code(), "signature");
    assert_eq!(error.offset(), 0);
}

/// Every strict prefix of the fixture fails with a truncation error: the
/// header, the index, a line and the terminator itself are all required.
#[test]
fn accept_f07_a_every_truncation_is_rejected() {
    for len in 0..SYNTHETIC.len() {
        let error =
            parse_error(read(&SYNTHETIC[..len]).expect_err("a truncated container must not parse"));
        assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "prefix {len}");
        assert!(error.field.starts_with("interp."), "prefix {len}: {error}");
    }
    // Losing only the terminator is reported at the terminator's field.
    let error = parse_error(read(&SYNTHETIC[..SYNTHETIC.len() - 4]).unwrap_err());
    assert_eq!(error.field, "interp.scripts[0].lines[2].size");
    assert_eq!(error.offset, 186);
}

/// A script offset past the end and a hostile script count are refused by
/// bounds checks, not by allocating.
#[test]
fn accept_f07_a_out_of_range_offset_and_hostile_count_are_refused() {
    let mut bytes = SYNTHETIC.to_vec();
    let offset_field = INTERP_HEADER_BYTES + NAME_FIELD_BYTES + 4;
    bytes[offset_field..offset_field + 4].copy_from_slice(&10_000u32.to_le_bytes());
    let error = parse_error(read(&bytes).unwrap_err());
    assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(error.field, "interp.scripts[0].offset");

    let mut bytes = SYNTHETIC.to_vec();
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = parse_error(read_interp(&mut context, &bytes).unwrap_err());
    assert_eq!(error.field, "interp.index");
    assert_eq!(context.allocation().used(), 0, "a refusal charges nothing");
}

// ---------------------------------------------------------------------------
// Stage F07-B: the validating lossless token decoder
// ---------------------------------------------------------------------------

/// A container laid out byte by byte: header and index first, then the test
/// writes script bodies at the offsets it chooses.
///
/// The index entries are declared up front and the bodies are written
/// afterwards, so a test can place a script inside the index, two entries on
/// one offset, or a gap between the index and the first script, and can leave
/// a script unterminated on purpose.
struct Image {
    bytes: Vec<u8>,
}

impl Image {
    /// Header and index for `(name field, declared script offset)` pairs. No
    /// bodies yet, so the image ends at the end of the index table.
    fn new(entries: &[(&[u8], u32)]) -> Self {
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, entries.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for (name_field, offset) in entries {
            let mut field = [0u8; NAME_FIELD_BYTES];
            field[..name_field.len()].copy_from_slice(name_field);
            bytes.extend_from_slice(&field);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        Self { bytes }
    }

    /// Writes `body` at `offset`, zero-filling whatever gap precedes it. A
    /// gap is an unclaimed region, which is how the tests produce one.
    fn put(&mut self, offset: u32, body: &[u8]) -> &mut Self {
        let offset = offset as usize;
        if self.bytes.len() < offset {
            self.bytes.resize(offset, 0);
        }
        self.bytes.truncate(offset);
        self.bytes.extend_from_slice(body);
        self
    }

    /// Grows the image to `len` bytes, leaving a tail after the last script.
    fn tail(&mut self, len: u32) -> &mut Self {
        self.bytes.resize(len as usize, 0);
        self
    }

    /// Sets the `script_offset` word of index entry `index`.
    fn declare(&mut self, index: usize, offset: u32) -> &mut Self {
        let at = INTERP_HEADER_BYTES + index * INDEX_ENTRY_BYTES + NAME_FIELD_BYTES + 4;
        self.bytes[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        self
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// The stored bytes of one line: `size`, `argument_count` and the data.
fn line(count: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

/// A script body: its lines and the zero `size` word that ends it.
fn body(lines: &[&[u8]]) -> Vec<u8> {
    let mut bytes: Vec<u8> = lines.iter().flat_map(|line| line.iter().copied()).collect();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

fn decode(bytes: &[u8]) -> Result<DecodedInterp<'_>, InterpError> {
    decode_interp(&mut ParseContext::with_defaults(CONTAINER), bytes)
}

/// The tokens of one decoded line as `(absolute offset, bytes)`.
fn tokens<'a>(script: &cs_formats::InterpScript<'a>, line: usize) -> Vec<(u64, &'a [u8])> {
    script
        .line(line)
        .expect("the line is in range")
        .tokens()
        .iter()
        .map(|token: &InterpToken<'a>| (token.offset(), token.bytes()))
        .collect()
}

/// The first index offset a script may occupy in a container with `count`
/// entries.
fn index_end(count: usize) -> u32 {
    (INTERP_HEADER_BYTES + count * INDEX_ENTRY_BYTES) as u32
}

// --- AC02, first half: a script must reach its terminator inside its extent.

/// A script whose body never terminates is refused at the offset where its
/// terminator should have been, with the neighbour's bytes left untouched.
///
/// The raw reader is the control: on the same bytes it happily decodes the
/// second script's line as the first script's second line, because nothing
/// bounds a script to its own extent. That is the behaviour F07-B removes, so
/// this test fails if the extent bound is dropped.
#[test]
fn accept_f07_b_missing_script_terminator_is_refused() {
    let first = line(1, b"a\0");
    let second = line(1, b"b\0");
    let start = index_end(2);
    // The first script's body has no terminator: its one line ends exactly
    // where the second script begins, so the byte the first script's
    // terminator would occupy is the second script's own `size` word.
    let neighbour = start + first.len() as u32;
    let mut image = Image::new(&[(b"swallow", start), (b"neighbour", neighbour)]);
    image.put(start, &first);
    image.put(neighbour, &body(&[&second]));
    let bytes = image.finish();

    // The control: the raw reader crosses the boundary.
    let raw = read(&bytes).expect("the raw reader is not extent-bounded");
    assert_eq!(raw.scripts()[0].lines.len(), 2, "it decoded the neighbour");
    assert_eq!(raw.scripts()[0].lines[1].data, b"b\0");
    assert_eq!(raw.scripts()[1].lines.len(), 1);

    // The validating decoder refuses at the first byte past the extent.
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("no terminator in the extent");
    assert_eq!(
        error,
        InterpError::Unterminated {
            container: CONTAINER.to_owned(),
            index: 0,
            offset: u64::from(neighbour),
            limit: u64::from(neighbour),
        }
    );
    assert_eq!(error.code(), "unterminated");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), u64::from(neighbour));
    assert_eq!(error.script(), Some(0));
    assert!(error.to_string().contains("unterminated"));
    assert_eq!(
        context.allocation().used(),
        0,
        "a refused decode charges nothing, so the same context can retry"
    );

    // A container that simply stops is a truncation, not an unterminated
    // script: the reader reports it, and the two are told apart.
    let mut truncated = Image::new(&[(b"short", index_end(1))]);
    truncated.put(index_end(1), &first);
    let bytes = truncated.finish();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("the terminator is missing");
    let error = match error {
        InterpError::Parse(error) => error,
        other => panic!("a container that ends is a truncation, got {other:?}"),
    };
    assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(error.field, "interp.scripts[0].lines[1].size");
    assert_eq!(context.allocation().used(), 0);
}

/// A line whose data runs past the end of its script's extent is refused with
/// the size it declared, and a script offset inside the index is refused
/// before any line is read.
#[test]
fn accept_f07_b_script_extents_are_validated_independently() {
    // A first script whose only line claims more data than the extent holds.
    let start = index_end(2);
    let mut image = Image::new(&[(b"overrun", start), (b"next", start + 4)]);
    let mut wide = Vec::new();
    wide.extend_from_slice(&100u32.to_le_bytes()); // size
    wide.extend_from_slice(&1u32.to_le_bytes()); // argument count
    image.put(start, &wide);
    image.tail(start + 4);
    let bytes = image.finish();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("the line crosses the extent");
    assert_eq!(
        error,
        InterpError::LineOverrun {
            container: CONTAINER.to_owned(),
            script: 0,
            line: 0,
            offset: u64::from(start),
            size: 100,
            limit: u64::from(start + 4),
        }
    );
    assert_eq!(error.code(), "line_overrun");
    assert_eq!(error.script(), Some(0));
    assert_eq!(context.allocation().used(), 0);

    // A script offset inside the header and index table: the two regions
    // would overlap, which is refused before a line is read.
    let start = index_end(1);
    let mut image = Image::new(&[(b"inside", start)]);
    image.put(start, &body(&[&line(1, b"a\0")]));
    image.declare(0, 12);
    let bytes = image.finish();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("the offset is inside the index");
    assert_eq!(
        error,
        InterpError::ScriptOffset {
            container: CONTAINER.to_owned(),
            index: 0,
            offset: 12,
            index_end: u64::from(start),
        }
    );
    assert_eq!(error.code(), "script_offset");
    assert_eq!(context.allocation().used(), 0);
}

/// An index whose entries are not in offset order still bounds each script by
/// the offsets the index declares, not by the order it lists them.
#[test]
fn accept_f07_b_extents_come_from_the_offsets_not_the_index_order() {
    let first = index_end(2);
    let second = first + body(&[&line(1, b"B\0")]).len() as u32;
    // Index entry 0 points at the *later* script.
    let mut image = Image::new(&[(b"late", second), (b"early", first)]);
    image.put(first, &body(&[&line(1, b"A\0")]));
    image.put(second, &body(&[&line(1, b"B\0")]));
    let bytes = image.finish();

    let decoded = decode(&bytes).expect("an unsorted index is still a valid index");
    assert_eq!(
        decoded
            .scripts()
            .iter()
            .map(|script| script.name())
            .collect::<Vec<_>>(),
        [b"late".as_slice(), b"early".as_slice()],
        "scripts keep index order"
    );
    let late = decoded.script(0).expect("index 0");
    let early = decoded.script(1).expect("index 1");
    assert_eq!(late.entry().script_offset, second);
    assert_eq!(early.entry().script_offset, first);
    assert_eq!(
        (early.limit(), late.limit()),
        (u64::from(second), u64::from(bytes.len() as u32)),
        "each script is bounded by the next offset, whichever entry declared it"
    );
    assert_eq!(tokens(early, 0), [(u64::from(first + 8), b"A".as_slice())]);
    assert_eq!(tokens(late, 0), [(u64::from(second + 8), b"B".as_slice())]);
    assert_eq!(early.len(), 1);
    assert_eq!(late.len(), 1);
    assert!(
        decoded.findings().is_empty(),
        "the scripts are back to back: {:?}",
        decoded.findings()
    );
}

// --- AC02, second half: the argument count must match the stored delimiters.

/// A line whose `argument_count` disagrees with the `0x00` delimiters in its
/// data is refused with both numbers, whether it declares too many or too few.
///
/// The raw reader accepts every one of these bytes and keeps the count word
/// verbatim, which is the F07-A contract; the disagreement is a validation
/// failure, not a parse failure.
#[test]
fn accept_f07_b_inconsistent_argument_count_is_refused() {
    let start = index_end(1);
    let data_offset = start + LINE_HEADER_BYTES as u32;

    for (declared, data, found) in [
        (3u32, &b"a\0b\0"[..], 2u32),
        (1, b"a\0b\0", 2),
        (0, b"\0", 1),
        (2, b"ab\0", 1),
        (u32::MAX, b"a\0", 1),
    ] {
        let bytes = {
            let mut image = Image::new(&[(b"counts", start)]);
            let mut one = line(declared, data);
            one.extend_from_slice(&0u32.to_le_bytes());
            image.put(start, &one);
            image.finish()
        };

        // The control: the raw reader keeps the count word and the data as
        // they are, whatever they disagree about.
        let raw = read(&bytes).expect("the raw reader does not check the count");
        assert_eq!(raw.scripts()[0].lines[0].argument_count, declared);

        let mut context = ParseContext::with_defaults(CONTAINER);
        let error = decode_interp(&mut context, &bytes)
            .expect_err("a count that disagrees with the delimiters is refused");
        assert_eq!(
            error,
            InterpError::ArgumentCount {
                container: CONTAINER.to_owned(),
                script: 0,
                line: 0,
                offset: u64::from(data_offset),
                declared,
                found,
            },
            "declared {declared} against {found} delimiters"
        );
        assert_eq!(error.code(), "argument_count");
        assert_eq!(error.script(), Some(0));
        assert!(
            !error.to_string().contains("a\0"),
            "argument bytes stay out of the error"
        );
        assert_eq!(context.allocation().used(), 0);
    }
}

/// Data that does not end with the delimiter closing its last argument is
/// refused: the end of that argument is stored nowhere in the file, so
/// decoding it would have to invent a boundary.
#[test]
fn accept_f07_b_unterminated_last_argument_is_refused() {
    let start = index_end(1);
    let mut image = Image::new(&[(b"tail", start)]);
    let mut stored = line(1, b"a\0b");
    stored.extend_from_slice(&0u32.to_le_bytes());
    image.put(start, &stored);
    let bytes = image.finish();

    // The control: the raw split yields "b" with no stored end.
    let raw = read(&bytes).expect("the raw reader does not require a final delimiter");
    let last = raw.scripts()[0].lines[0]
        .raw_arguments()
        .last()
        .expect("two arguments");
    assert_eq!((last.bytes, last.terminated), (b"b".as_slice(), false));

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("the last argument has no end");
    assert_eq!(
        error,
        InterpError::UnterminatedArguments {
            container: CONTAINER.to_owned(),
            script: 0,
            line: 0,
            offset: u64::from(start) + LINE_HEADER_BYTES as u64 + 3,
        }
    );
    assert_eq!(error.code(), "unterminated_arguments");
    assert_eq!(context.allocation().used(), 0);
}

// --- The decoded tokens themselves.

/// The golden fixture decodes to the tokens `expected.json` describes, each
/// pointing at its own bytes in the file, and re-joining them reproduces the
/// stored data exactly.
#[test]
fn accept_f07_b_golden_fixture_decodes_every_token_losslessly() {
    let mut context = ParseContext::with_defaults("fixtures/synthetic/synthetic.interp");
    let decoded = decode_interp(&mut context, SYNTHETIC).expect("the golden fixture validates");

    assert_eq!(decoded.header().signature, 144_118_041);
    assert_eq!(decoded.header().version, 7);
    assert_eq!(decoded.header().script_count, 1);
    assert_eq!(decoded.index_end(), 140);
    assert_eq!(decoded.container_len(), SYNTHETIC.len() as u64);
    assert_eq!(decoded.scripts().len(), 1);
    assert!(
        decoded.findings().is_empty(),
        "the fixture claims every byte: {:?}",
        decoded.findings()
    );

    let script = decoded.script(0).expect("one script");
    assert_eq!(script.name(), b"synthetic_probe");
    assert_eq!(script.entry().raw_timestamp, 0);
    assert_eq!(script.entry().entry_offset, 12);
    assert_eq!(script.limit(), SYNTHETIC.len() as u64);
    assert_eq!(script.len(), 2);

    // Line 0: "SYNTHETIC", "alpha", "beta" at 148, 158, 164.
    let first = script.line(0).expect("line 0");
    assert_eq!(
        tokens(script, 0),
        [
            (148, b"SYNTHETIC".as_slice()),
            (158, b"alpha".as_slice()),
            (164, b"beta".as_slice()),
        ]
    );
    assert_eq!(first.head().expect("a head token").bytes(), b"SYNTHETIC");
    // The raw record survives next to the tokens.
    assert_eq!(first.raw().offset, 140);
    assert_eq!(first.raw().size, 21);
    assert_eq!(first.data(), b"SYNTHETIC\0alpha\0beta\0");

    // Line 1: the two-argument line AC01 is about, byte for byte.
    let second = script.line(1).expect("line 1");
    assert_eq!(second.offset(), 169);
    assert_eq!(second.argument_count(), 2);
    assert_eq!(
        tokens(script, 1),
        [(177, b"VALUE".as_slice()), (183, b"42".as_slice())]
    );

    // Every token points at its own bytes in the fixture, and the tokens
    // rebuild the stored data with one 0x00 each.
    for line in script.lines() {
        assert_eq!(line.tokens().len(), line.argument_count() as usize);
        let mut rebuilt = Vec::new();
        for token in line.tokens() {
            let at = token.offset() as usize;
            assert_eq!(&SYNTHETIC[at..at + token.len()], token.bytes());
            rebuilt.extend_from_slice(token.bytes());
            rebuilt.push(0);
        }
        assert_eq!(rebuilt, line.data());
    }
}

/// Arguments that hold spaces, are empty or are not text keep their
/// boundaries and their bytes: `"a b","c"` and `"a","b c"` stay distinct, and
/// an argument that is a single high byte is not turned into a replacement
/// character.
#[test]
fn accept_f07_b_tokens_survive_spaces_empty_and_binary_arguments() {
    let start = index_end(1);
    // Each line is 8 bytes of header plus its data, and the data's first byte
    // is the first argument, so the offsets are derived from the same layout
    // the reader walks rather than counted by hand.
    let spaced = line(2, b"a b\0c\0");
    let joined = line(2, b"a\0b c\0");
    let empties = line(2, b"\0\0");
    let binary = line(1, b"\xff\x80\0");
    let step = LINE_HEADER_BYTES as u32;
    let second = start + spaced.len() as u32;
    let third = second + joined.len() as u32;
    let fourth = third + empties.len() as u32;
    let mut image = Image::new(&[(b"shapes", start)]);
    image.put(start, &body(&[&spaced, &joined, &empties, &binary]));
    let bytes = image.finish();

    let decoded = decode(&bytes).expect("every line is consistent");
    let script = decoded.script(0).expect("one script");
    assert_eq!(script.len(), 4);
    assert_eq!(
        tokens(script, 0),
        [
            (u64::from(start + step), b"a b".as_slice()),
            (u64::from(start + step + 4), b"c".as_slice())
        ]
    );
    assert_ne!(
        tokens(script, 0),
        tokens(script, 1),
        "the join would blur them"
    );
    assert_eq!(
        tokens(script, 1),
        [
            (u64::from(second + step), b"a".as_slice()),
            (u64::from(second + step + 2), b"b c".as_slice())
        ]
    );
    // Two empty arguments: the count is two, not zero.
    let empty = script.line(2).expect("line 2");
    assert_eq!(empty.len(), 2);
    assert!(empty.tokens().iter().all(|token| token.is_empty()));
    assert_eq!(empty.data(), b"\0\0");
    // A non-text argument stays the bytes that were stored.
    assert_eq!(
        tokens(script, 3),
        [(u64::from(fourth + step), b"\xff\x80".as_slice())]
    );
    // Every line's tokens rebuild its stored data, delimiters included.
    for line in script.lines() {
        let mut rebuilt = Vec::new();
        for token in line.tokens() {
            rebuilt.extend_from_slice(token.bytes());
            rebuilt.push(0);
        }
        assert_eq!(rebuilt, line.data(), "the split is lossless");
    }
}

/// Two scripts with equal names keep distinct origins through the validating
/// decoder, and an index that repeats an offset is reported rather than
/// collapsed: both entries decode with their own index position.
#[test]
fn accept_f07_b_equal_names_keep_distinct_origins() {
    let first = index_end(2);
    let second = first + body(&[&line(1, b"ONE\0")]).len() as u32;
    let mut image = Image::new(&[(b"twin", first), (b"twin", second)]);
    image.put(first, &body(&[&line(1, b"ONE\0")]));
    image.put(second, &body(&[&line(1, b"TWO\0")]));
    let bytes = image.finish();

    let decoded = decode(&bytes).expect("both scripts are valid");
    let [a, b] = decoded.scripts() else {
        panic!("two scripts expected")
    };
    assert_eq!(a.name(), b.name());
    assert_eq!((a.entry().index, b.entry().index), (0, 1));
    assert_eq!(
        (a.entry().entry_offset, b.entry().entry_offset),
        (12, 140),
        "the index entries stay distinct"
    );
    assert_eq!(
        (a.entry().script_offset, b.entry().script_offset),
        (first, second)
    );
    assert_eq!(tokens(a, 0), [(u64::from(first) + 8, b"ONE".as_slice())]);
    assert_eq!(tokens(b, 0), [(u64::from(second) + 8, b"TWO".as_slice())]);
    assert_eq!(a.entry().raw_timestamp, 0);

    // The same name, the same timestamp, one offset: two entries, one body.
    let mut image = Image::new(&[(b"shared", first), (b"shared", first)]);
    image.put(first, &body(&[&line(1, b"ONE\0")]));
    let bytes = image.finish();
    let decoded = decode(&bytes).expect("a shared offset is a finding, not a refusal");
    assert_eq!(decoded.scripts().len(), 2);
    assert_eq!(decoded.scripts()[0].lines(), decoded.scripts()[1].lines());
    assert_eq!(
        decoded.findings(),
        [cs_formats::InterpFinding::SharedScriptOffset {
            offset: first,
            entries: vec![0, 1],
        }]
    );
}

/// Bytes no script claims are reported, not skipped: the gap between the index
/// and the first script, the gap between two scripts and the tail after the
/// last one, each with its own offset and length.
#[test]
fn accept_f07_b_unclaimed_regions_are_retained_as_findings() {
    let first = index_end(2) + 12; // a 12-byte gap after the index
    let middle = body(&[&line(1, b"A\0")]);
    let second = first + middle.len() as u32 + 5; // a 5-byte gap between scripts
    let last = body(&[&line(1, b"B\0")]);
    let mut image = Image::new(&[(b"one", first), (b"two", second)]);
    image.put(first, &middle);
    image.put(second, &last);
    image.tail(second + last.len() as u32 + 5); // a 5-byte tail
    let bytes = image.finish();

    let decoded = decode(&bytes).expect("unclaimed bytes are findings, not failures");
    let found: Vec<(u64, u64)> = decoded
        .findings()
        .iter()
        .map(|finding| match finding {
            cs_formats::InterpFinding::Unclaimed { offset, length } => (*offset, *length),
            other => panic!("expected an unclaimed region, got {other:?}"),
        })
        .collect();
    assert_eq!(
        found,
        [
            (u64::from(index_end(2)), 12),
            (u64::from(first + middle.len() as u32), 5),
            (u64::from(second + last.len() as u32), 5),
        ],
        "every unclaimed stretch is located, in offset order"
    );
    for finding in decoded.findings() {
        assert_eq!(finding.code(), "unclaimed");
        assert!(finding.to_string().contains("unclaimed"));
    }
}

/// Name-field shapes this stage cannot judge are reported: a 120-byte field
/// with no `0x00` keeps the whole field as the name, and non-zero bytes after
/// the name's `0x00` are located rather than read as part of it.
#[test]
fn accept_f07_b_name_field_anomalies_are_reported_not_guessed() {
    let start = index_end(2);
    let unterminated = [b'x'; NAME_FIELD_BYTES];
    let second = start + body(&[&line(1, b"B\0")]).len() as u32;
    let mut image = Image::new(&[(&unterminated, start), (b"b", second)]);
    image.put(start, &body(&[&line(1, b"A\0")]));
    image.put(second, &body(&[&line(1, b"B\0")]));
    let bytes = image.finish();
    // Two non-zero padding bytes in entry 1's name field.
    let padding_at = INTERP_HEADER_BYTES + INDEX_ENTRY_BYTES + 5;
    let mut bytes = bytes;
    bytes[padding_at] = 0xAB;
    bytes[padding_at + 1] = 0xCD;

    let decoded = decode(&bytes).expect("a name field is not a reason to refuse a script");
    assert_eq!(
        decoded.script(0).expect("script 0").name(),
        &unterminated[..],
        "a full-width name is kept whole"
    );
    assert_eq!(decoded.script(1).expect("script 1").name(), b"b");
    assert_eq!(
        decoded.findings(),
        [
            cs_formats::InterpFinding::UnterminatedName {
                index: 0,
                offset: 12,
            },
            cs_formats::InterpFinding::NamePadding {
                index: 1,
                offset: padding_at as u64,
                length: 2,
            },
        ]
    );
    assert_eq!(decoded.findings()[0].code(), "unterminated_name");
    assert_eq!(decoded.findings()[1].code(), "name_padding");
}

/// Every decoded record is booked once, and a container that would not fit the
/// budget is refused before anything is charged.
#[test]
fn accept_f07_b_decoded_records_are_booked_once_and_refusals_charge_nothing() {
    let start = index_end(1);
    let mut image = Image::new(&[(b"budget", start)]);
    image.put(start, &body(&[&line(3, b"a\0b\0c\0"), &line(2, b"d\0e\0")]));
    let bytes = image.finish();

    // What the decoded container costs this parse.
    let mut probe = ParseContext::with_defaults(CONTAINER);
    let decoded = decode_interp(&mut probe, &bytes).expect("the container fits the default budget");
    let charge = probe.allocation().used();
    assert!(charge > 0, "the decoded tables are booked");
    assert_eq!(decoded.scripts()[0].len(), 2);
    assert_eq!(decoded.scripts()[0].line(0).expect("line 0").len(), 3);
    assert_eq!(decoded.scripts()[0].line(1).expect("line 1").len(), 2);

    // Exactly enough budget: the same container decodes and is charged once.
    let mut exact = ParseContext::new(CONTAINER, charge, 8);
    decode_interp(&mut exact, &bytes).expect("the exact budget fits");
    assert_eq!(exact.allocation().used(), charge);
    assert_eq!(exact.allocation().remaining(), 0);

    // One byte short: refused, and nothing is charged.
    let mut short = ParseContext::new(CONTAINER, charge - 1, 8);
    let error = decode_interp(&mut short, &bytes).expect_err("one byte short");
    let error = match error {
        InterpError::Parse(error) => error,
        other => panic!("a budget refusal is a parse failure, got {other:?}"),
    };
    assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(error.field, "interp.records");
    assert_eq!(short.allocation().used(), 0, "a refusal charges nothing");

    // The same context retries with a container it can hold: the refused
    // attempt left no charge behind, so the fit is still a fit.
    let small = {
        let mut image = Image::new(&[(b"small", index_end(1))]);
        image.put(index_end(1), &body(&[&line(1, b"a\0")]));
        image.finish()
    };
    decode_interp(&mut short, &small).expect("the retry fits what the refusal left");
    assert!(short.allocation().used() < charge);

    // Charges accumulate: a second full decode on the same context does not
    // fit any more.
    let mut twice = ParseContext::new(CONTAINER, charge * 2 - 1, 8);
    decode_interp(&mut twice, &bytes).expect("the first decode fits");
    let error = decode_interp(&mut twice, &bytes).expect_err("the second does not");
    assert_eq!(error.code(), "parse");
}
