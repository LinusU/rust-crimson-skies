//! Acceptance scenario F03-A (AC01): for every reader operation, truncate at
//! each byte boundary and assert an error, never a panic.
//!
//! These tests exercise production code only: `cs_formats::Reader` is the
//! reader parsers will use, and `common::parse_record` is the typed parse
//! entrypoint. Removing the bounds check from `Reader::take` (or the UTF-8 /
//! terminator checks) makes them fail, and a panic at any boundary aborts the
//! test rather than passing it.

mod common;

use common::{
    CONTAINER, COUNT_OFFSET, EXPECTED_COUNT, EXPECTED_FLAGS, EXPECTED_LABEL, EXPECTED_MAGIC,
    EXPECTED_SCALE, EXPECTED_VERSION, LABEL_LEN, parse_record, record_bytes, record_len,
};
use cs_formats::{ParseError, ParseErrorKind, Reader};

/// One reader operation plus how many bytes it must have to succeed.
struct OpCase {
    name: &'static str,
    field: &'static str,
    /// Bytes required from the current position for this operation to work.
    needed: usize,
    run: fn(&mut Reader<'_>) -> Result<(), ParseError>,
}

/// A 16-byte probe whose first 16 bytes satisfy every operation when read
/// from offset 0: valid UTF-8 ASCII with a `0x00` terminator at index 7.
fn probe_bytes() -> Vec<u8> {
    let mut probe = Vec::with_capacity(16);
    probe.extend_from_slice(b"abcdefg");
    probe.push(0);
    probe.extend_from_slice(b"hijklmno");
    probe
}

/// Every reader operation AC01 must cover.
fn op_cases() -> Vec<OpCase> {
    vec![
        OpCase {
            name: "read_u8",
            field: "probe.u8",
            needed: 1,
            run: |r| r.read_u8("probe.u8").map(|_| ()),
        },
        OpCase {
            name: "read_u16",
            field: "probe.u16",
            needed: 2,
            run: |r| r.read_u16("probe.u16").map(|_| ()),
        },
        OpCase {
            name: "read_u32",
            field: "probe.u32",
            needed: 4,
            run: |r| r.read_u32("probe.u32").map(|_| ()),
        },
        OpCase {
            name: "read_u64",
            field: "probe.u64",
            needed: 8,
            run: |r| r.read_u64("probe.u64").map(|_| ()),
        },
        OpCase {
            name: "read_i8",
            field: "probe.i8",
            needed: 1,
            run: |r| r.read_i8("probe.i8").map(|_| ()),
        },
        OpCase {
            name: "read_i16",
            field: "probe.i16",
            needed: 2,
            run: |r| r.read_i16("probe.i16").map(|_| ()),
        },
        OpCase {
            name: "read_i32",
            field: "probe.i32",
            needed: 4,
            run: |r| r.read_i32("probe.i32").map(|_| ()),
        },
        OpCase {
            name: "read_i64",
            field: "probe.i64",
            needed: 8,
            run: |r| r.read_i64("probe.i64").map(|_| ()),
        },
        OpCase {
            name: "read_f32",
            field: "probe.f32",
            needed: 4,
            run: |r| r.read_f32("probe.f32").map(|_| ()),
        },
        OpCase {
            name: "read_f64",
            field: "probe.f64",
            needed: 8,
            run: |r| r.read_f64("probe.f64").map(|_| ()),
        },
        OpCase {
            name: "read_bytes",
            field: "probe.bytes",
            needed: 6,
            run: |r| r.read_bytes("probe.bytes", 6).map(|_| ()),
        },
        OpCase {
            name: "skip",
            field: "probe.skip",
            needed: 9,
            run: |r| r.skip("probe.skip", 9),
        },
        OpCase {
            name: "sub_reader",
            field: "probe.sub",
            needed: 10,
            run: |r| r.sub_reader("probe.sub", 10).map(|_| ()),
        },
        OpCase {
            name: "read_str",
            field: "probe.str",
            needed: 8,
            run: |r| r.read_str("probe.str", 8).map(|_| ()),
        },
        OpCase {
            name: "read_bounded_cstr",
            field: "probe.cstr",
            needed: 16,
            run: |r| r.read_bounded_cstr("probe.cstr", 16).map(|_| ()),
        },
    ]
}

/// AC01: every reader operation, truncated at every byte boundary, returns a
/// structured error instead of panicking — and still succeeds when enough
/// bytes are present, so the assertion cannot pass by always failing.
#[test]
fn accept_f03_a_every_reader_operation_truncates_to_an_error() {
    let full = probe_bytes();
    assert_eq!(full.len(), 16);

    for case in op_cases() {
        for cut in 0..=full.len() {
            let mut reader = Reader::new(CONTAINER, &full[..cut]);
            let result = (case.run)(&mut reader);
            if cut < case.needed {
                match result {
                    Ok(()) => panic!(
                        "{} must return an error at cut {cut} (< {} bytes), but it succeeded",
                        case.name, case.needed
                    ),
                    Err(err) => assert_eq!(
                        err.kind,
                        ParseErrorKind::UnexpectedEof,
                        "{} at cut {cut}",
                        case.name
                    ),
                }
            } else if let Err(err) = result {
                panic!(
                    "{} must succeed with {} bytes available (cut = {cut}): {err}",
                    case.name, case.needed
                );
            }
        }
    }
}

/// AC01, per-operation error shape: a truncated read reports the container,
/// the absolute offset of the read, the field name and both conditions.
#[test]
fn accept_f03_a_truncation_error_retains_context() {
    let full = probe_bytes();
    for case in op_cases() {
        let mut reader = Reader::new(CONTAINER, &full[..case.needed - 1]);
        let err = (case.run)(&mut reader)
            .expect_err("one byte short must be an error, not a success or a panic");
        assert_eq!(err.kind, ParseErrorKind::UnexpectedEof, "{}", case.name);
        assert_eq!(err.container, CONTAINER, "{}", case.name);
        assert_eq!(err.offset, 0, "{}", case.name);
        assert_eq!(err.field, case.field, "{}", case.name);
        assert_eq!(
            err.expected,
            format!("{} bytes available", case.needed),
            "{}",
            case.name
        );
        assert_eq!(
            err.observed,
            format!("{} bytes available", case.needed - 1),
            "{}",
            case.name
        );
    }
}

/// AC01 for the whole record parse: truncating at each of the record's byte
/// boundaries yields an error naming the field that ran out of bytes, and the
/// complete record parses to the typed output.
#[test]
fn accept_f03_a_record_parse_truncates_at_each_byte_boundary() {
    let full = record_bytes();
    assert_eq!(full.len(), record_len());

    for cut in 0..full.len() {
        let mut reader = Reader::new(CONTAINER, &full[..cut]);
        let err = parse_record(&mut reader)
            .expect_err("a truncated record must not parse, and must not panic");
        assert_eq!(err.container, CONTAINER, "cut = {cut}");
        assert!(
            matches!(
                err.kind,
                ParseErrorKind::UnexpectedEof | ParseErrorKind::MissingTerminator
            ),
            "unexpected kind {:?} at cut {cut}",
            err.kind
        );
        assert!(
            err.offset <= cut as u64,
            "error offset {} must stay inside the truncated input ({cut})",
            err.offset
        );
    }

    // The observable failure named in the task description: cutting exactly
    // before `header.count` must report that field at its absolute offset.
    let cut = COUNT_OFFSET as usize;
    let mut reader = Reader::new(CONTAINER, &full[..cut]);
    let err = parse_record(&mut reader).expect_err("record truncated before header.count");
    assert_eq!(err.field, "header.count");
    assert_eq!(err.offset, COUNT_OFFSET);
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err.expected, "4 bytes available");
    assert_eq!(err.observed, "0 bytes available");

    // And the untruncated fixture parses to the exact typed values.
    let mut reader = Reader::new(CONTAINER, &full);
    let record = parse_record(&mut reader).expect("the complete record must parse");
    assert_eq!(record.magic, EXPECTED_MAGIC);
    assert_eq!(record.version, EXPECTED_VERSION);
    assert_eq!(record.label, EXPECTED_LABEL);
    assert_eq!(record.scale, EXPECTED_SCALE);
    assert_eq!(record.count, EXPECTED_COUNT);
    assert_eq!(record.flags, EXPECTED_FLAGS);
    assert!(
        reader.is_empty(),
        "the record must consume exactly its bytes"
    );
    assert_eq!(reader.position(), record_len() as u64);
    assert_eq!(reader.container(), CONTAINER);
}

/// The bounded-string failure cases: a field with no terminator inside its
/// bound is rejected, and invalid UTF-8 is rejected — both with structured
/// errors that carry no payload bytes.
#[test]
fn accept_f03_a_string_bounds_report_errors_without_payload() {
    // No 0x00 anywhere inside the 8-byte bound.
    let mut no_nul = record_bytes();
    no_nul[6..6 + LABEL_LEN].copy_from_slice(b"SECRET!!");
    let mut reader = Reader::new(CONTAINER, &no_nul);
    let err = parse_record(&mut reader).expect_err("a label without a terminator must be rejected");
    assert_eq!(err.kind, ParseErrorKind::MissingTerminator);
    assert_eq!(err.field, "header.label");
    assert_eq!(err.offset, 6);
    let rendered = err.to_string();
    assert!(
        !rendered.contains("SECRET"),
        "the error must not carry file payload: {rendered}"
    );

    // Terminated but not valid UTF-8 (0xFF is never valid).
    let mut bad_utf8 = record_bytes();
    bad_utf8[6..6 + LABEL_LEN].copy_from_slice(b"bad\xFF\0abc");
    let mut reader = Reader::new(CONTAINER, &bad_utf8);
    let err = parse_record(&mut reader).expect_err("invalid UTF-8 must be rejected");
    assert_eq!(err.kind, ParseErrorKind::InvalidEncoding);
    assert_eq!(err.field, "header.label");
    assert_eq!(err.offset, 9, "the offset points at the offending byte");
    let rendered = err.to_string();
    assert!(
        !rendered.contains("bad") && !rendered.contains("\u{FF}"),
        "the error must name an index, not the bytes: {rendered}"
    );

    // `read_str` keeps the same rule for plain fixed strings.
    let mut reader = Reader::new(CONTAINER, &[b'a', b'b', 0xFF, b'd']);
    let err = reader
        .read_str("blob.text", 4)
        .expect_err("invalid UTF-8 must be rejected");
    assert_eq!(err.kind, ParseErrorKind::InvalidEncoding);
    assert_eq!(err.offset, 2);
}
