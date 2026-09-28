//! Acceptance scenario F03-B (AC02): a `u32::MAX` count and an
//! `offset + length` overflow are refused through the bounded-allocation
//! utilities without allocating anything.
//!
//! Both refusals are counter arithmetic in [`AllocationBudget`] or in the
//! checked extent helper of [`Reader`]: nothing is charged, no buffer is
//! built and the reader's input slice is untouched. The observable failure
//! pinned here is the structured error that appears while the checks run —
//! removing the budget charge makes `reserve` succeed and these assertions
//! fail. `accept_f03_b_no_large_allocation.rs` measures the "without
//! allocation" half with a counting allocator.

use cs_formats::{AllocationBudget, ParseErrorKind, Reader};

/// Provenance label carried by every error these tests assert on.
const CONTAINER: &str = "synthetic/f03_b_budget.bin";

/// AC02, first half: a `u32::MAX` count times 8 bytes is 32 GiB — beyond the
/// default budget, so it is refused without ever becoming an allocation.
#[test]
fn accept_f03_b_u32_max_count_is_refused_by_the_default_budget() {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    let limit = AllocationBudget::DEFAULT_LIMIT;
    let product = u32::MAX as u64 * 8;

    // The reservation is attempted first; which structured error comes back
    // depends on whether the product fits this target's `usize`.
    let outcome = budget.reserve("table.count", 18, u32::MAX as u64, 8);
    match usize::try_from(product) {
        Ok(bytes) => {
            let err =
                outcome.expect_err("u32::MAX * 8 bytes is 32 GiB, far past the default budget");
            assert!(bytes as u64 > limit, "the fixture must exceed the budget");
            assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
            assert_eq!(err.container, CONTAINER);
            assert_eq!(err.offset, 18, "the error anchors on the caller's position");
            assert_eq!(err.field, "table.count");
            assert_eq!(
                err.expected,
                format!("{limit} of {limit} allocation-budget bytes available")
            );
            assert_eq!(err.observed, format!("{bytes} bytes requested"));
            assert_eq!(
                budget.used(),
                0,
                "a refused reservation must not consume budget"
            );
            assert_eq!(budget.remaining(), limit);
        }
        Err(_) => {
            // Narrow target: the product itself must be refused instead.
            let err = outcome.expect_err("the product must overflow on a narrow usize");
            assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
            assert_eq!(err.field, "table.count");
            assert_eq!(budget.used(), 0);
        }
    }
}

/// Products that overflow are length errors carrying container, offset and
/// field, charged to nothing — checked before any slice or allocation.
#[test]
fn accept_f03_b_overflowing_products_are_length_overflows() {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);

    let err = budget
        .reserve("table.count", 4, u64::MAX, 8)
        .expect_err("u64::MAX * 8 overflows");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 4);
    assert_eq!(err.field, "table.count");
    assert_eq!(err.expected, "count * element_size to fit in usize");
    assert_eq!(
        err.observed,
        format!("count {} times element_size 8", u64::MAX)
    );

    let err = budget
        .reserve("table.count", 4, u32::MAX as u64, u64::MAX)
        .expect_err("u32::MAX * u64::MAX overflows");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.field, "table.count");
    assert_eq!(
        budget.used(),
        0,
        "a refused reservation must not consume budget"
    );
}

/// AC02, second half: an `offset + length` overflow is refused without
/// allocation, through the budget's extent check and through the reader's.
#[test]
fn accept_f03_b_offset_plus_length_overflow_is_refused_without_allocation() {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    let err = budget
        .reserve_extent("member.range", u64::MAX - 3, 8)
        .expect_err("(u64::MAX - 3) + 8 overflows a u64 extent");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(
        err.offset,
        u64::MAX - 3,
        "the error anchors on the range's own start offset"
    );
    assert_eq!(err.field, "member.range");
    assert_eq!(err.expected, "offset + length to fit in u64");
    assert_eq!(
        err.observed,
        format!("offset {} plus length 8", u64::MAX - 3)
    );
    assert_eq!(budget.used(), 0, "the overflow must charge nothing");

    // The reader keeps the same shape with its own provenance, and consumes
    // no bytes: the check is arithmetic over the range, not a slice.
    let reader = Reader::new(CONTAINER, &[0u8; 8]);
    let err = reader
        .checked_extent("member.range", u64::MAX - 3, 8)
        .expect_err("(u64::MAX - 3) + 8 overflows for the reader too");
    assert_eq!(err.kind, ParseErrorKind::LengthOverflow);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.field, "member.range");
    assert_eq!(
        reader.position(),
        0,
        "a refused check must not consume bytes"
    );
    assert_eq!(reader.remaining(), 8, "the input slice must stay untouched");
}

/// Within-budget requests are accepted and charged, so the utilities bound
/// honest parses instead of refusing everything.
#[test]
fn accept_f03_b_within_budget_reservations_succeed_and_charge() {
    let mut budget = AllocationBudget::new(CONTAINER, 64);

    let bytes = budget
        .reserve("table.count", 0, 8, 4)
        .expect("8 * 4 bytes fit in a 64-byte budget");
    assert_eq!(bytes, 32);
    assert_eq!(budget.used(), 32);

    let end = budget
        .reserve_extent("member.range", 16, 32)
        .expect("32 more bytes fit in a 64-byte budget");
    assert_eq!(end, 48, "the extent check returns the exclusive end offset");
    assert_eq!(budget.used(), 64);
    assert_eq!(budget.remaining(), 0, "the budget is now exactly exhausted");
}
