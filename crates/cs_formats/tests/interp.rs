//! Acceptance stage F07-A: raw INTERP records and the golden synthetic
//! fixture (`specs/F07-interp-loading-script-container.md`, section
//! `### F07-A`).
//!
//! Every byte in this file is authored here, except
//! `fixtures/synthetic/synthetic.interp` and `bad-version.interp`, whose
//! bytes are authored by `tools/make_synthetic_fixtures.py` and whose
//! expected values are asserted independently of this crate's reader
//! (`fixtures/synthetic/expected.json`). No original game data, no
//! `CS_GAME_DIR` access.

use cs_formats::{
    INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, InterpError, InterpFile, LINE_HEADER_BYTES,
    NAME_FIELD_BYTES, ParseContext, ParseErrorKind, RawArgument, read_interp,
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
