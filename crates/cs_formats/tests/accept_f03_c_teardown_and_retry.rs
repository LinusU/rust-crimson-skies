//! Acceptance scenario F03-C: teardown and retry of parser entrypoints.
//!
//! A failed attempt must leave the context exactly as it found it — recursion
//! depth released, allocation charges rolled back — so the same context can
//! retry the same bytes honestly. A successful attempt must keep its charges,
//! and neither a hostile attempt nor a ceiling refusal may widen a limit.

mod common;

use common::{CONTAINER, EXPECTED_COUNT, parse_record, record_bytes};
use cs_formats::{ParseContext, ParseError, ParseErrorKind};

/// A failing attempt tears everything down, and the same context then parses
/// the intact record: the retry is charged for its own reservations only.
#[test]
fn accept_f03_c_failed_attempts_tear_down_and_can_be_retried() {
    let mut context = ParseContext::new(CONTAINER, 1024, 4);

    // Attempt 1: reserves allocation bytes, descends two levels, then fails
    // on a short read. Everything it took must be released with it.
    let short = &record_bytes()[..4];
    let err = context
        .parse("record", short, |reader, allocation, recursion| {
            assert_eq!(allocation.reserve("table", 0, 4, 16)?, 64);
            let _outer = recursion.enter("node", 0)?;
            let _inner = recursion.enter("node", 1)?;
            reader.read_u64("header.tail")?;
            Ok::<(), ParseError>(())
        })
        .expect_err("4 bytes cannot satisfy an 8-byte read");
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.field, "record.header.tail");
    assert_eq!(err.expected, "8 bytes available");
    assert_eq!(err.observed, "4 bytes available");
    assert_eq!(
        context.recursion().depth(),
        0,
        "both guards were released on the error path"
    );
    assert_eq!(
        context.allocation().used(),
        0,
        "the failed attempt's 64 bytes were rolled back"
    );

    // Retry with the same context and the intact record.
    let (record, charged) = context
        .parse(
            "record",
            &record_bytes(),
            |reader, allocation, recursion| {
                let record = parse_record(reader)?;
                let charged = allocation.reserve("table", reader.position(), 4, 16)?;
                let _guard = recursion.enter("node", 0)?;
                Ok::<_, ParseError>((record, charged))
            },
        )
        .expect("the same context must retry successfully");
    assert_eq!(record.count, EXPECTED_COUNT);
    assert_eq!(charged, 64);
    assert_eq!(
        context.allocation().used(),
        64,
        "only the successful attempt is charged — no double booking"
    );
    assert_eq!(context.recursion().depth(), 0);
    assert_eq!(context.allocation().limit(), 1024, "limits never move");
}

/// One hundred failed attempts that each take the whole budget and a nesting
/// level before failing must leave no trace: the hundred-and-first honest
/// attempt still fits its reservation exactly.
#[test]
fn accept_f03_c_hostile_attempts_never_drain_the_budget() {
    let mut context = ParseContext::new(CONTAINER, 64, 2);
    for attempt in 0..100 {
        let err = context
            .parse(
                "record",
                &record_bytes()[..4],
                |reader, allocation, recursion| {
                    assert_eq!(allocation.reserve("table", 0, 64, 1)?, 64);
                    let _guard = recursion.enter("node", 0)?;
                    reader.read_u64("header.tail")?;
                    Ok::<(), ParseError>(())
                },
            )
            .expect_err("the short read must fail");
        assert_eq!(err.field, "record.header.tail");
        assert_eq!(
            context.allocation().used(),
            0,
            "attempt {attempt} left no charge behind"
        );
        assert_eq!(
            context.recursion().depth(),
            0,
            "attempt {attempt} left no nesting level behind"
        );
        assert_eq!(context.allocation().limit(), 64, "the limit never moves");
    }

    context
        .parse(
            "record",
            &record_bytes(),
            |_reader, allocation, _recursion| {
                allocation.reserve("table", 0, 64, 1)?;
                Ok::<(), ParseError>(())
            },
        )
        .expect("100 failed attempts must not have drained a 64-byte budget");
    assert_eq!(context.allocation().used(), 64);
    assert_eq!(context.recursion().depth(), 0);
}

/// The recursion ceiling is enforced inside an attempt with the same
/// provenance and scoping as every other refusal, and the guards taken before
/// the refusal are released when it propagates out.
#[test]
fn accept_f03_c_recursion_ceiling_is_scoped_and_released() {
    let mut context = ParseContext::new(CONTAINER, 1024, 2);
    let err = context
        .parse(
            "record",
            &record_bytes(),
            |_reader, _allocation, recursion| {
                let _first = recursion.enter("node", 0)?;
                let _second = recursion.enter("node", 1)?;
                recursion.enter("node", 2)?;
                Ok::<(), ParseError>(())
            },
        )
        .expect_err("level 3 exceeds a 2-level ceiling");
    assert_eq!(err.kind, ParseErrorKind::RecursionDepthExceeded);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.field, "record.node");
    assert_eq!(err.offset, 2, "the anchor the descent was made at");
    assert_eq!(err.expected, "at most 2 nested levels");
    assert_eq!(err.observed, "level 3 requested");
    assert_eq!(
        context.recursion().depth(),
        0,
        "the two guards taken before the refusal were released"
    );

    context
        .parse(
            "record",
            &record_bytes(),
            |_reader, _allocation, recursion| {
                let _first = recursion.enter("node", 0)?;
                let _second = recursion.enter("node", 1)?;
                Ok::<(), ParseError>(())
            },
        )
        .expect("a retry within the ceiling must succeed");
    assert_eq!(context.recursion().depth(), 0);
}

/// Rollback applies to failed attempts only: what a successful attempt
/// reserved stays booked, and the ledger accumulates across attempts.
#[test]
fn accept_f03_c_successful_attempts_keep_their_charges() {
    let mut context = ParseContext::new(CONTAINER, 1024, 2);
    context
        .parse(
            "record",
            &record_bytes(),
            |_reader, allocation, _recursion| {
                assert_eq!(allocation.reserve("table", 0, 3, 16)?, 48);
                Ok::<(), ParseError>(())
            },
        )
        .expect("the first attempt succeeds");
    assert_eq!(
        context.allocation().used(),
        48,
        "a successful attempt is never rolled back"
    );

    context
        .parse(
            "record",
            &record_bytes(),
            |_reader, allocation, _recursion| {
                assert_eq!(allocation.reserve("table", 48, 1, 16)?, 16);
                Ok::<(), ParseError>(())
            },
        )
        .expect("the second attempt succeeds");
    assert_eq!(
        context.allocation().used(),
        64,
        "charges accumulate across successful attempts"
    );
    assert_eq!(context.allocation().remaining(), 1024 - 64);
}
