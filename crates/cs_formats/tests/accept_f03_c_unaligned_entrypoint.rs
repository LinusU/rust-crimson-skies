//! Acceptance scenario F03-C: deliberately unaligned slices parsed through
//! the contextual-error entrypoint (`ParseContext::parse`), plus the AC01
//! truncation sweep driven through that same entrypoint.
//!
//! The entrypoint hands the parser a byte-wise little-endian reader, so an
//! input whose bytes sit at odd addresses must parse identically to an
//! aligned one, and every truncation of it must come back as a scoped
//! [`cs_formats::ParseError`] — never a panic and never an unscoped field.

mod common;

use common::{
    CONTAINER, EXPECTED_COUNT, EXPECTED_LABEL, EXPECTED_MAGIC, EXPECTED_SCALE, parse_record,
    record_bytes, record_len,
};
use cs_formats::{ParseContext, ParseErrorKind};

/// AC03: a record placed at an odd address (so `u16`/`u32`/`f32` fields are
/// misaligned) parses to exactly the same typed values through
/// `ParseContext::parse` as it does through a bare reader. A transmute-based
/// reader would fault or decode garbage here.
#[test]
fn accept_f03_c_unaligned_slices_parse_through_the_entrypoint() {
    let mut backing: Vec<u8> = Vec::with_capacity(1 + record_len());
    // Decide the pad length from the allocation's own address: the first
    // record byte must land on an odd address whatever the allocator
    // returned, so the test cannot silently run aligned on a target whose
    // `Vec<u8>` happens to start at an odd address.
    let pad = 1 - (backing.as_ptr() as usize % 2);
    backing.resize(pad, 0x5A);
    backing.extend_from_slice(&record_bytes());

    let slice = &backing[pad..];
    assert_eq!(
        slice.as_ptr() as usize % 2,
        1,
        "the fixture must actually be unaligned for this test to mean anything"
    );
    assert_eq!(slice.len(), record_len(), "the slice is exactly the record");

    let mut context = ParseContext::with_defaults(CONTAINER);
    let record = context
        .parse("record", slice, |reader, _allocation, _recursion| {
            parse_record(reader)
        })
        .expect("an unaligned record must parse through the entrypoint");
    assert_eq!(record.magic, EXPECTED_MAGIC);
    assert_eq!(record.label, EXPECTED_LABEL);
    assert_eq!(record.count, EXPECTED_COUNT);
    assert!((record.scale - EXPECTED_SCALE).abs() < f32::EPSILON);
    assert_eq!(context.container(), CONTAINER);
    assert_eq!(
        context.allocation().used(),
        0,
        "reading records charges no allocation budget"
    );
    assert_eq!(
        context.recursion().depth(),
        0,
        "reading records enters no nesting level"
    );
}

/// AC01 through the F03-C entrypoint: truncate the unaligned record at every
/// byte boundary. Each cut must produce an error naming the container, the
/// absolute offset of the missing field, the field path scoped by the
/// entrypoint, and the expected/observed byte counts — never a panic.
#[test]
fn accept_f03_c_unaligned_truncation_at_every_boundary_is_scoped() {
    let mut backing: Vec<u8> = Vec::with_capacity(1 + record_len());
    let pad = 1 - (backing.as_ptr() as usize % 2);
    backing.resize(pad, 0x5A);
    backing.extend_from_slice(&record_bytes());
    let full = &backing[pad..];
    assert_eq!(
        full.as_ptr() as usize % 2,
        1,
        "the fixture must be unaligned"
    );

    let mut context = ParseContext::with_defaults(CONTAINER);
    for cut in 0..record_len() {
        // magic: 0..4, version: 4..6, label: 6..14, scale: 14..18,
        // count: 18..22, flags: 22..23 — a cut inside one of them makes the
        // next read the one that fails.
        let (field, offset, needed): (&str, u64, usize) = match cut {
            0..=3 => ("header.magic", 0, 4),
            4..=5 => ("header.version", 4, 2),
            6..=13 => ("header.label", 6, 8),
            14..=17 => ("header.scale", 14, 4),
            18..=21 => ("header.count", 18, 4),
            _ => ("header.flags", 22, 1),
        };

        let err = match context.parse("record", &full[..cut], |reader, _allocation, _recursion| {
            parse_record(reader)
        }) {
            Ok(_) => panic!("the record truncated to {cut} bytes must not parse"),
            Err(err) => err,
        };

        assert_eq!(
            err.kind,
            ParseErrorKind::UnexpectedEof,
            "cut {cut} ({field}) must fail on bounds, not on something else"
        );
        assert_eq!(err.container, CONTAINER, "cut {cut} keeps provenance");
        assert_eq!(err.offset, offset, "cut {cut} reports the absolute offset");
        assert_eq!(
            err.field,
            format!("record.{field}"),
            "cut {cut} is scoped by the entrypoint it crossed"
        );
        assert_eq!(
            err.expected,
            format!("{needed} bytes available"),
            "cut {cut} names what the field needs"
        );
        assert_eq!(
            err.observed,
            format!("{} bytes available", cut as u64 - offset),
            "cut {cut} names what was actually there"
        );
    }
}
