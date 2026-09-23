//! Acceptance scenario F03-A: deliberately unaligned slices and nested range
//! provenance (AC03, plus the sub-reader half of AC01).
//!
//! The reader decodes byte-wise with explicit little-endian conversions, so an
//! input whose bytes sit at odd addresses must parse identically to an aligned
//! one, and a failure inside a nested range must keep its absolute offset.

mod common;

use common::{CONTAINER, EXPECTED_LABEL, EXPECTED_MAGIC, parse_record, record_bytes, record_len};
use cs_formats::{ParseErrorKind, Reader};

/// AC03: a record placed at an odd address (so `u16`/`u32`/`f32` fields are
/// misaligned) parses to exactly the same typed values. A `transmute`-based
/// reader would fault or decode garbage here.
#[test]
fn accept_f03_a_reads_deliberately_unaligned_slices() {
    let mut backing = Vec::with_capacity(1 + record_len());
    backing.push(0x5A); // odd prefix: every record byte shifts off alignment
    backing.extend_from_slice(&record_bytes());

    let slice = &backing[1..];
    assert_eq!(
        slice.as_ptr() as usize % 2,
        1,
        "the fixture must actually be unaligned for this test to mean anything"
    );

    let mut reader = Reader::new(CONTAINER, slice);
    let record = parse_record(&mut reader).expect("an unaligned record must parse");
    assert_eq!(record.magic, EXPECTED_MAGIC);
    assert_eq!(record.label, EXPECTED_LABEL);
    assert_eq!(record.count, common::EXPECTED_COUNT);
    assert!((record.scale - common::EXPECTED_SCALE).abs() < f32::EPSILON);

    // Truncation still errors (rather than panics) on an unaligned slice.
    let mut reader = Reader::new(CONTAINER, &slice[..3]);
    let err = reader
        .read_u32("header.magic")
        .expect_err("an unaligned truncated read must still be an error");
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err.offset, 0);
}

/// Nested ranges keep container provenance and rebase offsets: an error
/// raised inside a sub-reader reports its absolute offset in the container,
/// not an offset local to the sub-range.
#[test]
fn accept_f03_a_sub_reader_errors_report_absolute_offsets() {
    let mut bytes = vec![0u8; 5];
    bytes.extend_from_slice(&record_bytes());

    // Truncated container: the member range starts at offset 5 but has only
    // 6 bytes, so reading a u64 inside it must fail at absolute offset 5.
    let mut outer = Reader::new(CONTAINER, &bytes[..11]);
    outer.skip("prefix", 5).expect("5 prefix bytes are present");
    let mut inner = outer
        .sub_reader("member", 6)
        .expect("6 member bytes are present");
    assert_eq!(inner.position(), 5);
    assert_eq!(inner.container(), CONTAINER);

    let err = inner
        .read_u64("member.seed")
        .expect_err("8 bytes are not available inside the 6-byte member");
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(
        err.container, CONTAINER,
        "the archive name must survive nesting"
    );
    assert_eq!(err.offset, 5, "offsets are absolute, not sub-range local");
    assert_eq!(err.field, "member.seed");
    assert_eq!(err.expected, "8 bytes available");
    assert_eq!(err.observed, "6 bytes available");

    // The full member parses through the nested reader and advances the outer
    // reader past it.
    let mut outer = Reader::new(CONTAINER, &bytes);
    outer.skip("prefix", 5).expect("5 prefix bytes are present");
    let mut inner = outer
        .sub_reader("member", record_len())
        .expect("the member range fits");
    let record = parse_record(&mut inner).expect("the nested record must parse");
    assert_eq!(record.magic, EXPECTED_MAGIC);
    assert!(inner.is_empty());
    assert_eq!(inner.position(), (5 + record_len()) as u64);
    assert_eq!(
        outer.remaining(),
        0,
        "the member consumed exactly its range"
    );

    // A nested range that does not fit is rejected before any slicing.
    let mut outer = Reader::new(CONTAINER, &bytes);
    let err = outer
        .sub_reader("member", record_len() + 6)
        .expect_err("a range larger than the remaining bytes must be rejected");
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err.field, "member");
    assert_eq!(err.offset, 0);
}
