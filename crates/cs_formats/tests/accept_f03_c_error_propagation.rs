//! Acceptance scenario F03-C: contextual errors at every parser entrypoint.
//!
//! Two entrypoints are crossed here — the top-level `ParseContext::parse` and
//! a nested helper that scopes its own failures — and the failure that comes
//! out must still name the container, the absolute offset, the field path
//! from the outside in, and the expected/observed conditions. Budget refusals
//! raised inside the same attempt must carry the very same provenance as
//! reader refusals.

mod common;

use common::{CONTAINER, COUNT_OFFSET, record_bytes};
use cs_formats::{ParseContext, ParseError, ParseErrorKind, Reader};

/// A nested parser entrypoint: it reads `header.count` and, as its failures
/// cross it, contributes its own scope to the field path — exactly what
/// `ParseContext::parse` does for the outer name.
fn read_entry(reader: &mut Reader<'_>) -> Result<u32, ParseError> {
    reader
        .read_u32("header.count")
        .map_err(|error| error.in_scope("entries[3]"))
}

/// A failure raised by a nested entrypoint inside `ParseContext::parse` keeps
/// the container, the absolute offset and the conditions, and its field path
/// reads outside in: `record.entries[3].header.count`.
#[test]
fn accept_f03_c_entrypoint_failures_keep_context_and_scope() {
    let bytes = record_bytes();
    // Cut exactly where `header.count` starts, so that read is the one that
    // fails with 0 of 4 bytes available.
    let short = &bytes[..COUNT_OFFSET as usize];

    let mut context = ParseContext::with_defaults(CONTAINER);
    let err = context
        .parse("record", short, |reader, _allocation, _recursion| {
            // Consume the fields that precede `header.count` so the failing
            // read is the one this entrypoint is named after.
            reader.skip("header.preamble", COUNT_OFFSET as usize)?;
            read_entry(reader)
        })
        .expect_err("`header.count` is absent from the truncated record");

    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(
        err.container, CONTAINER,
        "provenance survives both entrypoints"
    );
    assert_eq!(
        err.offset, COUNT_OFFSET,
        "the offset stays absolute and untouched by scoping"
    );
    assert_eq!(err.field, "record.entries[3].header.count");
    assert_eq!(err.expected, "4 bytes available");
    assert_eq!(err.observed, "0 bytes available");

    let rendered = err.to_string();
    assert!(rendered.contains(CONTAINER), "display keeps the container");
    assert!(
        rendered.contains("record.entries[3].header.count"),
        "display keeps the scoped field path: {rendered}"
    );

    // An empty scope adds nothing instead of a stray separator.
    let bare = err.clone().in_scope("");
    assert_eq!(bare.field, "record.entries[3].header.count");
    // Scoping is purely additive: container and offset never move.
    let deeper = err.clone().in_scope("archive");
    assert_eq!(deeper.container, CONTAINER);
    assert_eq!(deeper.offset, COUNT_OFFSET);
    assert_eq!(deeper.field, "archive.record.entries[3].header.count");
}

/// An allocation refusal raised *inside* the attempt reports the same
/// container as the reader does, is scoped by the entrypoint, and charges
/// nothing: the ledger ends the failed attempt exactly where it started.
#[test]
fn accept_f03_c_budget_failures_share_the_entrypoint_provenance() {
    let mut context = ParseContext::new(CONTAINER, 64, 2);
    let err = context
        .parse(
            "record",
            &record_bytes(),
            |reader, allocation, _recursion| {
                let anchor = reader.position();
                // Four 16-byte entries fit the 64-byte budget exactly.
                assert_eq!(allocation.reserve("table.small", anchor, 4, 16)?, 64);
                // The fifth does not.
                allocation.reserve("table.entries", anchor, 5, 16)?;
                Ok::<(), ParseError>(())
            },
        )
        .expect_err("a 80-byte request must exceed a 64-byte budget");

    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(
        err.container, CONTAINER,
        "budget errors name the same archive as reader errors"
    );
    assert_eq!(err.field, "record.table.entries");
    assert_eq!(err.offset, 0, "the anchor the reservation was made at");
    assert_eq!(err.expected, "0 of 64 allocation-budget bytes available");
    assert_eq!(err.observed, "80 bytes requested");

    assert_eq!(
        context.allocation().used(),
        0,
        "the exact fit was charged, the refusal was not, and the failed \
         attempt rolled its own charges back"
    );
    assert_eq!(context.recursion().depth(), 0);
}
