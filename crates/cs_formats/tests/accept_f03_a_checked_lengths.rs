//! Acceptance scenario F03-A: checked length arithmetic (AC02's arithmetic
//! half) — `u32::MAX` counts and `offset + length` overflow are rejected
//! without allocating anything.
//!
//! `Reader` never owns buffer memory, so the proof that no allocation happens
//! is structural: `checked_byte_len`/`checked_extent` are pure arithmetic and
//! `read_bytes` only hands out a borrow of the input, whatever the length.

mod common;

use common::CONTAINER;
use cs_formats::{ParseErrorKind, Reader};

/// `u32::MAX` entries of 8 bytes must not overflow the length computation, and
/// the resulting byte count must be refused by the reader without allocating
/// a buffer of that size.
#[test]
fn accept_f03_a_u32_max_count_is_bounded_without_allocation() {
    let mut reader = Reader::new(CONTAINER, &[0u8; 4]);
    let total = u32::MAX as u64 * 8;

    match usize::try_from(total) {
        Ok(len) => {
            let reported = reader
                .checked_byte_len("table.count", u32::MAX as u64, 8)
                .expect("u32::MAX * 8 must not overflow a usize on this target");
            assert_eq!(reported, len);

            let err = reader
                .read_bytes("table.entries", len)
                .expect_err("4 bytes cannot hold u32::MAX * 8 entries");
            assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(err.field, "table.entries");
            assert_eq!(err.container, CONTAINER);
            assert_eq!(err.expected, format!("{len} bytes available"));
            assert_eq!(err.observed, "4 bytes available");
            assert_eq!(
                reader.position(),
                0,
                "a refused read must not consume any bytes"
            );
            assert_eq!(
                reader.remaining(),
                4,
                "the input slice must be untouched after the refusal"
            );
        }
        Err(_) => {
            // Narrow target: the product itself must be refused instead.
            let err = reader
                .checked_byte_len("table.count", u32::MAX as u64, 8)
                .expect_err("the product must overflow on a narrow usize");
            assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
        }
    }
}

/// Products and extents that overflow are structured length errors carrying
/// container, offset and field — checked before any slice or allocation.
#[test]
fn accept_f03_a_overflowing_lengths_are_structured_errors() {
    let mut reader = Reader::new(CONTAINER, &[0u8; 4]);
    reader.skip("prefix", 2).expect("2 bytes are present");

    let err = reader
        .checked_byte_len("table.count", u64::MAX, 8)
        .expect_err("u64::MAX * 8 overflows");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 2, "the error points at the current position");
    assert_eq!(err.field, "table.count");
    assert_eq!(err.expected, "count * element_size to fit in usize");
    assert_eq!(
        err.observed,
        format!("count {} times element_size 8", u64::MAX)
    );

    let err = reader
        .checked_byte_len("table.count", u32::MAX as u64, u64::MAX)
        .expect_err("u32::MAX * u64::MAX overflows");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.field, "table.count");

    let err = reader
        .checked_extent("member.range", u64::MAX - 3, 8)
        .expect_err("(u64::MAX - 3) + 8 overflows");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 2);
    assert_eq!(err.field, "member.range");
    assert_eq!(err.expected, "offset + length to fit in u64");
    assert_eq!(
        err.observed,
        format!("offset {} plus length 8", u64::MAX - 3)
    );

    // The non-overflowing counterpart still returns the end offset.
    assert_eq!(
        reader
            .checked_extent("member.range", u64::MAX - 8, 8)
            .expect("an exactly-fitting extent must be accepted"),
        u64::MAX
    );
}
