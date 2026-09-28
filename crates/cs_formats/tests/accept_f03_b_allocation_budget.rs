//! Acceptance scenario F03-B: the allocation budget is an *independent*
//! limit (spec non-negotiable #2) with exact boundaries and a designed
//! default — configurable only where these tests reach.
//!
//! Every case below calls production code (`AllocationBudget`); removing the
//! charge in `AllocationBudget::charge` makes the boundary and exhaustion
//! cases fail, and removing the counter makes the accounting cases fail.

use cs_formats::{AllocationBudget, ParseErrorKind};

/// Provenance label carried by every error these tests assert on.
const CONTAINER: &str = "synthetic/f03_b_budget.bin";

/// The budget admits a request that fits exactly and refuses the next byte,
/// without charging the refused request.
#[test]
fn accept_f03_b_budget_boundary_is_exact() {
    let mut budget = AllocationBudget::new(CONTAINER, 64);

    let bytes = budget
        .reserve("row.bytes", 0, 16, 4)
        .expect("16 * 4 bytes fit exactly in a 64-byte budget");
    assert_eq!(bytes, 64);
    assert_eq!(budget.used(), 64);
    assert_eq!(budget.remaining(), 0);

    let err = budget
        .reserve("row.bytes", 64, 1, 1)
        .expect_err("one byte past an exhausted budget must be refused");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 64);
    assert_eq!(err.field, "row.bytes");
    assert_eq!(err.expected, "0 of 64 allocation-budget bytes available");
    assert_eq!(err.observed, "1 bytes requested");
    assert_eq!(
        budget.used(),
        64,
        "the refused request must not consume budget"
    );

    // A range request is bounded by the same remaining-byte accounting.
    let err = budget
        .reserve_extent("member.range", 64, 1)
        .expect_err("an exhausted budget refuses ranges too");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(budget.used(), 64);
}

/// The low end of the configuration surface: a zero budget refuses
/// everything, including a zero-length request's neighbours.
#[test]
fn accept_f03_b_zero_budget_refuses_every_request() {
    let mut budget = AllocationBudget::new(CONTAINER, 0);

    let err = budget
        .reserve("table.count", 0, 1, 1)
        .expect_err("a zero budget admits no bytes at all");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(err.expected, "0 of 0 allocation-budget bytes available");
    assert_eq!(err.observed, "1 bytes requested");

    let err = budget
        .reserve_extent("member.range", 0, 1)
        .expect_err("an exhausted budget refuses ranges too");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(budget.used(), 0);
    assert_eq!(budget.limit(), 0);

    // A zero-length request charges nothing, so it is admitted even here.
    assert_eq!(
        budget
            .reserve_extent("member.range", 0, 0)
            .expect("an empty range allocates no bytes"),
        0
    );
}

/// Budgets are independent: draining one changes nothing about another, and
/// a clone starts fresh accounting rather than inheriting a widened limit.
#[test]
fn accept_f03_b_budgets_are_independent() {
    let mut drained = AllocationBudget::new(CONTAINER, 64);
    drained
        .reserve("row.bytes", 0, 64, 1)
        .expect("the whole budget is available at first");

    let mut other = AllocationBudget::new(CONTAINER, 64);
    assert_eq!(
        other.used(),
        0,
        "one exhausted budget must not drain another"
    );
    other
        .reserve("row.bytes", 0, 64, 1)
        .expect("the second budget is untouched by the first");

    let fresh = drained.clone();
    assert_eq!(fresh.used(), 64, "a clone keeps its own accounting");
    let mut fresh = fresh;
    let err = fresh
        .reserve("row.bytes", 0, 1, 1)
        .expect_err("a clone carries the used bytes, not a wider limit");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);

    let widened = AllocationBudget::new(CONTAINER, 128);
    assert_eq!(
        widened.used(),
        0,
        "an explicit new limit starts at zero use"
    );
}

/// The designed default is exact: one buffer of `DEFAULT_LIMIT` bytes is
/// admitted, the byte after it is not, and a `u32::MAX` count is admitted
/// only by an explicitly larger budget — never by a hardcoded cap.
#[test]
fn accept_f03_b_default_budget_admits_one_buffer_and_refuses_the_next() {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    let limit = AllocationBudget::DEFAULT_LIMIT;
    let quarter = limit / 4;

    let bytes = budget
        .reserve("texture.pixels", 0, quarter, 4)
        .expect("a buffer of exactly the default limit fits");
    assert_eq!(bytes as u64, limit);
    assert_eq!(budget.remaining(), 0);

    let err = budget
        .reserve("texture.pixels", limit, 1, 1)
        .expect_err("the byte after the default limit is refused");
    assert_eq!(err.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert_eq!(
        err.expected,
        format!("0 of {limit} allocation-budget bytes available")
    );
    assert_eq!(budget.used(), limit);

    // The high end of the configuration surface: `u32::MAX` entries of one
    // byte fit a budget that explicitly allows them.
    let mut large = AllocationBudget::new(CONTAINER, u64::MAX);
    let bytes = large
        .reserve("table.count", 0, u32::MAX as u64, 1)
        .expect("u32::MAX bytes fit a budget of u64::MAX bytes");
    assert_eq!(bytes, u32::MAX as usize);
    assert_eq!(large.used(), u32::MAX as u64);
}
