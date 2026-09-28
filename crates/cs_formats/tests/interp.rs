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

/// The header the decoder shares with the raw reader stays hostile-input safe:
/// a `script_count` it cannot back with an index table and an index table
/// that stops mid-entry are both refusals rather than buffers, an offset past
/// the end of the container never becomes a line, and a container that indexes
/// no scripts decodes to nothing with its tail reported rather than dropped.
#[test]
fn accept_f07_b_hostile_headers_and_out_of_range_offsets_are_refused() {
    // A script_count no file can back: the index table is refused before it
    // is borrowed, and the refused attempt charges nothing.
    for count in [u32::MAX, 2, 1 << 20] {
        // One whole entry plus eight bytes of a second one, so the index runs
        // out inside an entry for every count of two or more.
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, count] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut field = [0u8; NAME_FIELD_BYTES];
        field[..1].copy_from_slice(b"a");
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&140u32.to_le_bytes());
        bytes.extend_from_slice(&[0xAB; 8]);
        let mut context = ParseContext::with_defaults(CONTAINER);
        let error = decode_interp(&mut context, &bytes).expect_err("a count with no index");
        assert_eq!(error.code(), "parse", "count {count}");
        let error = match error {
            InterpError::Parse(error) => error,
            other => panic!("count {count} gave {other:?}"),
        };
        assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "count {count}");
        assert_eq!(error.field, "interp.index", "count {count}");
        assert_eq!(context.allocation().used(), 0, "count {count}");
    }

    // A complete index whose scripts are not in the bytes at all: the entries
    // parse, and the first missing line is where the refusal lands.
    let bytes = Image::new(&[(b"a", index_end(2)), (b"b", index_end(2) + 16)]).finish();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("no script bodies");
    assert_eq!(error.code(), "parse");
    let error = match error {
        InterpError::Parse(error) => error,
        other => panic!("a container with no bodies gave {other:?}"),
    };
    assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(error.field, "interp.scripts[0].lines[0].size");
    assert_eq!(context.allocation().used(), 0);

    // A script offset past the end of the container is refused where the
    // reader is asked to seek, not read as a line.
    let start = index_end(1);
    let mut image = Image::new(&[(b"far", 10_000)]);
    image.put(start, &body(&[&line(1, b"a\0")]));
    let bytes = image.finish();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = decode_interp(&mut context, &bytes).expect_err("the offset is past the end");
    assert_eq!(error.code(), "parse");
    let error = match error {
        InterpError::Parse(error) => error,
        other => panic!("an offset past the end gave {other:?}"),
    };
    assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(error.field, "interp.scripts[0].offset");
    assert_eq!(context.allocation().used(), 0);

    // A container that indexes no scripts decodes to nothing, and the bytes
    // after its index are reported rather than dropped.
    let mut empty = Image::new(&[]).finish();
    let decoded = decode(&empty).expect("a container with no scripts is valid");
    assert_eq!(decoded.scripts().len(), 0);
    assert_eq!(decoded.index_end(), INTERP_HEADER_BYTES as u64);
    assert!(decoded.findings().is_empty());
    empty.extend_from_slice(&[0xAB; 9]);
    let decoded = decode(&empty).expect("a tail does not make it invalid");
    assert_eq!(
        decoded.findings(),
        [cs_formats::InterpFinding::Unclaimed {
            offset: INTERP_HEADER_BYTES as u64,
            length: 9,
        }]
    );
}

// ---------------------------------------------------------------------------
// Stage F07-C: the loading plan
// ---------------------------------------------------------------------------

use cs_formats::{
    KeyArguments, KeyPart, KeySpelling, LoadCommand, LoadCommandTable, MalformedKey, PlanLineKind,
    PlanStats, ScriptOrigin, TableError, plan_interp_loading,
};
use cs_types::evidence::ClaimStatus;

/// One line's arguments as a data block: the argument count equals the `0x00`
/// delimiters, which is what the decoder requires.
fn args<'a>(tokens: impl IntoIterator<Item = &'a [u8]>) -> (u32, Vec<u8>) {
    let mut data = Vec::new();
    let mut count = 0u32;
    for token in tokens {
        data.extend_from_slice(token);
        data.push(0);
        count += 1;
    }
    (count, data)
}

/// A registration a test supplies. Every test in this section states its
/// claim status honestly: `Designed` for a rule invented to exercise the
/// machinery, and nothing higher.
fn registration(spelling: &[u8], namespace: usize, path: usize) -> LoadCommand {
    LoadCommand {
        spelling: spelling.to_vec(),
        arguments: KeyArguments {
            namespace,
            path,
            variant: None,
        },
        spelling_kind: KeySpelling::Literal,
        status: ClaimStatus::Designed,
        source: "synthetic test table: exercises the plan, not an original command".to_owned(),
    }
}

fn plan_of<'a>(bytes: &'a [u8], table: &LoadCommandTable) -> cs_formats::InterpLoadPlan<'a> {
    let decoded = decode(bytes).expect("the container validates");
    plan_interp_loading(&decoded, table)
}

/// AC03 on the plan: two scripts with equal names — the same name, the same
/// `timestamp` and even the same body — keep distinct origins in the plan, so
/// a consumer can tell them apart without inventing an identity out of the
/// name or the timestamp (non-negotiable #5).
#[test]
fn accept_f07_c_equal_names_keep_distinct_origins() {
    let first = index_end(2);
    let second = first + body(&[&line(1, b"ONE\0")]).len() as u32;
    let mut image = Image::new(&[(b"twin", first), (b"twin", second)]);
    image.put(first, &body(&[&line(1, b"ONE\0")]));
    image.put(second, &body(&[&line(1, b"TWO\0")]));
    let mut bytes = image.finish();
    // The same timestamp in both entries: the plan must not use it as identity.
    for entry in 0..2 {
        let at = INTERP_HEADER_BYTES + entry * INDEX_ENTRY_BYTES + NAME_FIELD_BYTES;
        bytes[at..at + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    }

    let table = LoadCommandTable::new();
    let plan = plan_of(&bytes, &table);
    assert_eq!(plan.scripts().len(), 2);
    let [a, b] = plan.scripts() else {
        panic!("two scripts expected")
    };
    assert_eq!(a.name(), b.name(), "the names are equal");
    assert_eq!(a.raw_timestamp(), b.raw_timestamp());
    assert_eq!(a.raw_timestamp(), 0x1234_5678);

    // The origins differ in every field a consumer can point at, so the plan
    // never has to fall back on the name to say which script is which.
    let origin_a: ScriptOrigin = a.origin();
    let origin_b: ScriptOrigin = b.origin();
    assert_ne!(origin_a, origin_b);
    assert_eq!((origin_a.index(), origin_b.index()), (0, 1));
    assert_eq!(
        (origin_a.entry_offset(), origin_b.entry_offset()),
        (12, 140)
    );
    assert_eq!(
        (origin_a.script_offset(), origin_b.script_offset()),
        (first, second)
    );
    // Each origin names the byte range a consumer hashes for identity, and the
    // two ranges are the two different bodies: `size`, `argument_count`,
    // "ONE\0" and the terminator, then the same shape around "TWO\0".
    let expected_first = body(&[&line(1, b"ONE\0")]);
    let expected_second = body(&[&line(1, b"TWO\0")]);
    assert_eq!(
        origin_a.end(),
        u64::from(first) + expected_first.len() as u64
    );
    assert_eq!(
        origin_b.end(),
        u64::from(second) + expected_second.len() as u64
    );
    assert_eq!(
        &bytes[first as usize..origin_a.end() as usize],
        expected_first
    );
    assert_eq!(
        &bytes[second as usize..origin_b.end() as usize],
        expected_second
    );
    // The two ranges are disjoint and the scripts keep index order.
    assert!(origin_a.end() <= u64::from(second));
    assert_eq!(second, first + expected_first.len() as u32);

    // Two entries pointing at one offset really are one body, and the plan
    // says so through the offsets rather than by collapsing them.
    let mut shared = Image::new(&[(b"twin", first), (b"twin", first)]);
    shared.put(first, &body(&[&line(1, b"ONE\0")]));
    let shared = shared.finish();
    let plan = plan_of(&shared, &table);
    assert_eq!(plan.scripts().len(), 2);
    assert_ne!(plan.scripts()[0].origin(), plan.scripts()[1].origin());
    assert_eq!(
        plan.scripts()[0].origin().script_offset(),
        plan.scripts()[1].origin().script_offset()
    );
}

/// An empty table classifies nothing: every line is unclassified, the plan is
/// incomplete, and the blocking lines carry the offsets a consumer reports.
/// This is the state every container is in until F07-D measures the real
/// commands, and it must fail the plan rather than assume the lines load
/// nothing.
#[test]
fn accept_f07_c_unregistered_command_is_unclassified_with_its_offset() {
    let (count, data) = args([b"LoadGameGen".as_slice(), b"models/plane.flt", b"plane"]);
    let start = index_end(1);
    let mut image = Image::new(&[(b"load", start)]);
    image.put(start, &body(&[&line(count, &data)]));
    let bytes = image.finish();

    let plan = plan_of(&bytes, &LoadCommandTable::new());
    assert!(!plan.is_complete());
    assert_eq!(plan.stats().scripts, 1);
    assert_eq!(plan.stats().lines, 1);
    assert_eq!(plan.stats().unclassified_commands, 1);
    assert_eq!(plan.stats().loading_commands, 0);
    assert_eq!(plan.stats().malformed_commands, 0);
    assert_eq!(plan.stats().blocked_scripts, 1);
    assert_eq!(plan.stats().distinct_heads, 1);
    assert_eq!(plan.blocked_scripts().count(), 1);
    assert!(
        plan.commands().is_empty(),
        "the plan carries the table it used"
    );

    let script = plan.script(0).expect("one script");
    assert!(!script.is_usable());
    let blocking = script.blocking_lines().collect::<Vec<_>>();
    assert_eq!(blocking.len(), 1);
    let entry = blocking[0];
    assert_eq!(*entry.kind(), PlanLineKind::Unclassified);
    assert_eq!(entry.kind().code(), "unclassified");
    assert_eq!(entry.head().bytes(), b"LoadGameGen");
    // The offsets a diagnostic needs: the line's `size` word and the head
    // token's first byte inside it.
    assert_eq!(entry.source_offset(), u64::from(start));
    assert_eq!(entry.head_offset(), u64::from(start) + 8);
    assert_eq!(
        &bytes[entry.head_offset() as usize..entry.head_offset() as usize + 11],
        b"LoadGameGen"
    );
}

/// A registered command is matched by exact bytes and its key is read from the
/// argument positions the registration names. Registration is not
/// interpretation: a `Composed` key is carried through as a token the consumer
/// cannot resolve, and the claim's status and source travel with the plan.
#[test]
fn accept_f07_c_registered_command_spellings_its_key_from_named_arguments() {
    let (count, data) = args([b"loadmesh".as_slice(), b"world", b"c1/plane.flt", b"hi"]);
    let start = index_end(1);
    let mut image = Image::new(&[(b"load", start)]);
    image.put(start, &body(&[&line(count, &data)]));
    let bytes = image.finish();

    let mut table = LoadCommandTable::new();
    let mut with_variant = registration(b"loadmesh", 1, 2);
    with_variant.arguments.variant = Some(3);
    with_variant.status = ClaimStatus::ObservedTool;
    with_variant.source = "synthetic test table: a rule with a variant".to_owned();
    let composed = LoadCommand {
        spelling: b"loadmesh".to_vec(),
        arguments: KeyArguments {
            namespace: 1,
            path: 2,
            variant: None,
        },
        spelling_kind: KeySpelling::Composed,
        status: ClaimStatus::Inferred,
        source: "synthetic test table: the same spelling, composed".to_owned(),
    };
    table.insert(with_variant).expect("the rule registers");
    assert_eq!(
        table.insert(composed.clone()),
        Err(TableError::Duplicate {
            first: 0,
            second: 1
        })
    );

    let plan = plan_of(&bytes, &table);
    assert!(plan.is_complete());
    assert_eq!(plan.stats().loading_commands, 1);
    assert_eq!(plan.stats().unclassified_commands, 0);
    assert_eq!(plan.stats().blocked_scripts, 0);

    // The plan keeps the table it was built with, so a report cannot be read
    // against a different table later.
    assert_eq!(plan.commands().len(), 1);
    let rule = plan.command(0).expect("the snapshot holds the rule");
    assert_eq!(rule.spelling, b"loadmesh");
    assert_eq!(rule.arguments.variant, Some(3));
    assert_eq!(rule.status, ClaimStatus::ObservedTool);
    assert_eq!(rule.spelling_kind.code(), "literal");

    let entry = plan
        .script(0)
        .expect("one script")
        .line(0)
        .expect("one line");
    assert!(!entry.is_blocking());
    let PlanLineKind::Loading { command, key } = *entry.kind() else {
        panic!("expected a loading line, got {:?}", entry.kind())
    };
    assert_eq!(command, 0);
    assert_eq!(key.namespace().bytes(), b"world");
    assert_eq!(key.path().bytes(), b"c1/plane.flt");
    assert_eq!(
        key.variant().map(|token| token.bytes()),
        Some(b"hi".as_slice())
    );
    // Each part keeps the absolute offset of its own bytes: the registration
    // names position 1 for the namespace, 2 for the path and 3 for the
    // variant, so the head token at position 0 is not among them.
    let data_start = u64::from(start) + 8;
    assert_eq!(
        &bytes[data_start as usize..data_start as usize + 8],
        b"loadmesh"
    );
    assert_eq!(key.namespace().offset(), data_start + 9);
    assert_eq!(key.path().offset(), data_start + 15);
    assert_eq!(
        key.variant().map(|token| token.offset()),
        Some(data_start + 28)
    );
    assert_eq!(
        &bytes[key.path().offset() as usize..key.path().offset() as usize + 12],
        b"c1/plane.flt"
    );
    assert!(plan.script(0).expect("one script").is_usable());
    assert_eq!(plan.blocked_scripts().count(), 0);
}

/// A registered command whose stored arguments do not match its registration
/// is `Malformed`, with the position that is missing or empty — never padded,
/// skipped, or read out of a neighbouring argument.
#[test]
fn accept_f07_c_mismatched_arguments_are_malformed_not_repaired() {
    let start = index_end(1);
    let mut table = LoadCommandTable::new();
    table
        .insert(registration(b"loadmesh", 1, 2))
        .expect("the rule registers");
    let mut with_variant = registration(b"loadvariant", 1, 2);
    with_variant.arguments.variant = Some(3);
    table.insert(with_variant).expect("the rule registers");

    /// One case: the head spelling, the stored arguments and what the
    /// registration should say about them.
    type Case<'a> = (&'a [u8], Vec<Vec<u8>>, MalformedKey);

    let cases: [Case<'_>; 4] = [
        (
            b"loadmesh",
            vec![b"loadmesh".to_vec()],
            MalformedKey::MissingArgument { position: 1 },
        ),
        (
            b"loadmesh",
            vec![b"loadmesh".to_vec(), b"".to_vec(), b"a.flt".to_vec()],
            MalformedKey::EmptyArgument { position: 1 },
        ),
        (
            b"loadmesh",
            vec![b"loadmesh".to_vec(), b"world".to_vec()],
            MalformedKey::MissingArgument { position: 2 },
        ),
        (
            b"loadvariant",
            vec![
                b"loadvariant".to_vec(),
                b"world".to_vec(),
                b"a.flt".to_vec(),
            ],
            MalformedKey::MissingArgument { position: 3 },
        ),
    ];

    for (spelling, tokens, expected) in cases {
        let (count, data) = args(tokens.iter().map(Vec::as_slice));
        let mut image = Image::new(&[(b"load", start)]);
        image.put(start, &body(&[&line(count, &data)]));
        let bytes = image.finish();
        let plan = plan_of(&bytes, &table);
        assert!(!plan.is_complete(), "{spelling:?} with {tokens:?}");
        assert_eq!(plan.stats().loading_commands, 0);
        assert_eq!(plan.stats().malformed_commands, 1);
        assert_eq!(plan.stats().unclassified_commands, 0);
        let entry = plan
            .script(0)
            .and_then(|script| script.line(0))
            .expect("one script with one line");
        let PlanLineKind::Malformed { command, reason } = *entry.kind() else {
            panic!(
                "expected a malformed line for {spelling:?}, got {:?}",
                entry.kind()
            )
        };
        assert_eq!(reason, expected, "{spelling:?} with {tokens:?}");
        assert_eq!(reason.code(), expected.code());
        assert_eq!(reason.position(), expected.position());
        assert!(
            reason
                .to_string()
                .contains(&expected.position().to_string())
        );
        // The failure names the registration that did not match, so a
        // consumer can find the rule to correct.
        let rule = plan
            .command(command)
            .expect("the plan kept its rule snapshot");
        assert_eq!(rule.spelling, spelling);
        assert_eq!(entry.head().bytes(), spelling);
    }
}

/// The table refuses the registrations that would make classification
/// ambiguous, untraceable or self-awarding, and it refuses them *before* any
/// plan exists, so a rejected table never produces a plan at all.
#[test]
fn accept_f07_c_command_table_refuses_ambiguous_registrations() {
    let mut table = LoadCommandTable::new();
    assert!(table.is_empty());
    assert_eq!(table.get(b"loadmesh"), None);

    let mut empty = registration(b"", 1, 2);
    empty.source = "x".to_owned();
    assert_eq!(table.insert(empty), Err(TableError::EmptySpelling));

    let mut unsourced = registration(b"loadmesh", 1, 2);
    unsourced.source = "   ".to_owned();
    assert_eq!(table.insert(unsourced), Err(TableError::EmptySource));

    let mut self_awarded = registration(b"loadmesh", 1, 2);
    self_awarded.status = ClaimStatus::VerifiedOriginal;
    assert_eq!(
        table.insert(self_awarded),
        Err(TableError::SelfAwardedVerifiedOriginal)
    );

    for (part, arguments) in [
        (
            KeyPart::Namespace,
            KeyArguments {
                namespace: 0,
                path: 2,
                variant: None,
            },
        ),
        (
            KeyPart::Path,
            KeyArguments {
                namespace: 1,
                path: 0,
                variant: None,
            },
        ),
        (
            KeyPart::Variant,
            KeyArguments {
                namespace: 1,
                path: 2,
                variant: Some(0),
            },
        ),
    ] {
        let mut rule = registration(b"loadmesh", 1, 2);
        rule.arguments = arguments;
        assert_eq!(table.insert(rule), Err(TableError::HeadArgument { part }));
    }
    for (part, arguments) in [
        (
            KeyPart::Path,
            KeyArguments {
                namespace: 1,
                path: 1,
                variant: None,
            },
        ),
        (
            KeyPart::Variant,
            KeyArguments {
                namespace: 1,
                path: 2,
                variant: Some(2),
            },
        ),
        (
            KeyPart::Variant,
            KeyArguments {
                namespace: 2,
                path: 3,
                variant: Some(2),
            },
        ),
    ] {
        let mut rule = registration(b"loadmesh", 1, 2);
        rule.arguments = arguments;
        assert_eq!(
            table.insert(rule),
            Err(TableError::RepeatedArgument {
                part,
                position: arguments.variant.unwrap_or(arguments.path)
            })
        );
    }

    // None of the refused registrations was kept, so the table is still empty
    // and still classifies nothing.
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);

    // A whole set applies or none of it does.
    assert_eq!(
        table.extend([
            registration(b"loadmesh", 1, 2),
            registration(b"loadmesh", 2, 3)
        ]),
        Err(TableError::Duplicate {
            first: 0,
            second: 1
        })
    );
    assert!(table.is_empty());
    table
        .extend([
            registration(b"loadmesh", 1, 2),
            registration(b"loadother", 1, 3),
        ])
        .expect("two distinct rules apply");
    assert_eq!(table.len(), 2);
    assert_eq!(table.get(b"loadmesh").map(|(index, _)| index), Some(0));
    assert_eq!(table.get(b"loadother").map(|(index, _)| index), Some(1));
    assert_eq!(
        table.get(b"loadMesh"),
        None,
        "matching is byte-exact, not folded"
    );
    assert_eq!(table.get(b"load"), None, "a prefix does not match");
}

/// The stats are a tally of what was read, decoded and not understood, summed
/// over every script — the shape `docs/contracts/SCRIPT-MISSION.md` asks a
/// source adapter for. A container with one classified line among unclassified
/// ones reports both.
#[test]
fn accept_f07_c_stats_tally_every_script_and_head() {
    let (a_count, a_data) = args([b"loadmesh".as_slice(), b"world", b"a.flt"]);
    let (b_count, b_data) = args([b"loadmesh".as_slice(), b"world", b"b.flt"]);
    let (c_count, c_data) = args([b"quit".as_slice()]);
    // The second script's offset is derived from the first script's whole
    // stored body, so the two scripts are back to back and nothing is
    // unclaimed.
    let first_body = body(&[&line(a_count, &a_data), &line(c_count, &c_data)]);
    let second_body = body(&[&line(b_count, &b_data)]);
    let first = index_end(2);
    let second = first + first_body.len() as u32;
    let mut image = Image::new(&[(b"one", first), (b"two", second)]);
    image.put(first, &first_body);
    image.put(second, &second_body);
    let bytes = image.finish();
    assert!(decode(&bytes).expect("valid").findings().is_empty());

    let mut table = LoadCommandTable::new();
    table
        .insert(registration(b"loadmesh", 1, 2))
        .expect("registered");
    let plan = plan_of(&bytes, &table);
    let stats = plan.stats();
    assert_eq!(stats.scripts, 2);
    assert_eq!(stats.lines, 3);
    assert_eq!(stats.loading_commands, 2);
    assert_eq!(stats.unclassified_commands, 1);
    assert_eq!(stats.malformed_commands, 0);
    assert_eq!(stats.distinct_heads, 2, "loadmesh and quit");
    assert_eq!(
        stats.blocked_scripts, 1,
        "only the script with `quit` is blocked"
    );
    assert!(!plan.is_complete());
    assert_eq!(plan.blocked_scripts().count(), 1);
    assert_eq!(
        plan.blocked_scripts().next().map(|s| s.name()),
        Some(&b"one"[..])
    );
    assert_eq!(
        stats.lines,
        stats.loading_commands + stats.unclassified_commands + stats.malformed_commands
    );
    assert!(plan.script(1).expect("two scripts").is_usable());

    // A container with no scripts plans to nothing and is complete: there is
    // nothing unclassified to fail on.
    let empty = Image::new(&[]).finish();
    let plan = plan_of(&empty, &table);
    assert!(plan.is_complete());
    assert_eq!(
        plan.stats(),
        PlanStats {
            scripts: 0,
            ..plan.stats()
        }
    );
    assert_eq!(plan.stats().lines, 0);
    assert_eq!(plan.stats().blocked_scripts, 0);
}

/// The plan is a read of the container: it never resolves, executes or repairs
/// anything. A registered command whose key is spelled literally still leaves
/// the resolution, the world and the dependencies to the consumer, and the
/// plan says nothing about them.
#[test]
fn accept_f07_c_plan_reads_the_container_without_resolving_or_executing() {
    // A `..` component and a `%VAR%` reference: neither is repaired, dropped
    // nor resolved here, and the plan keeps both tokens exactly.
    let (count, data) = args([b"loadmesh".as_slice(), b"world", b"..\\data\\c1.flt"]);
    let start = index_end(1);
    let mut image = Image::new(&[(b"load", start)]);
    image.put(start, &body(&[&line(count, &data)]));
    let bytes = image.finish();

    let mut table = LoadCommandTable::new();
    table
        .insert(registration(b"loadmesh", 1, 2))
        .expect("registered");
    let plan = plan_of(&bytes, &table);
    assert!(plan.is_complete());
    let entry = plan.script(0).and_then(|s| s.line(0)).expect("one line");
    let PlanLineKind::Loading { key, .. } = *entry.kind() else {
        panic!("expected a loading line")
    };
    assert_eq!(key.path().bytes(), b"..\\data\\c1.flt");
    assert_eq!(key.variant(), None);
    // The whole line is still there, losslessly, for the consumer to judge.
    assert_eq!(entry.line().data(), b"loadmesh\0world\0..\\data\\c1.flt\0");
    assert_eq!(entry.line().argument_count(), 3);
    let rebuilt: Vec<u8> = entry
        .line()
        .tokens()
        .iter()
        .flat_map(|token| {
            let mut bytes = token.bytes().to_vec();
            bytes.push(0);
            bytes
        })
        .collect();
    assert_eq!(rebuilt, entry.line().data());
}
