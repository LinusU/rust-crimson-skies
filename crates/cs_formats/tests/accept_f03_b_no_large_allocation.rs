//! Acceptance scenario F03-B (AC02's "without allocation" half), measured
//! rather than argued: a counting allocator records every allocation made
//! while the refusals run, and the test fails if any of them is large enough
//! to be the buffer the hostile lengths ask for.
//!
//! The refusal path still allocates the small `String`s inside a
//! [`cs_formats::ParseError`] (container, field, expected/observed text) —
//! those grow with fixed labels, never with file content. What must never
//! appear is a buffer sized from the input: `u32::MAX * 8` is 32 GiB.
//!
//! The binary carries exactly one test, so no other test thread can move the
//! counters while the measured section runs; setup happens before the
//! counters are zeroed and assertions read them before formatting.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_formats::{AllocationBudget, ParseErrorKind, Reader, RecursionBudget};

/// Provenance label carried by every error these tests assert on.
const CONTAINER: &str = "synthetic/f03_b_no_alloc.bin";

/// No single allocation inside the measured section may reach this size.
/// Error strings stay in the tens of bytes; the refused request is 32 GiB.
const MAX_SINGLE_ALLOC: u64 = 64 * 1024;

static LARGEST_ALLOC: AtomicU64 = AtomicU64::new(0);
static TOTAL_ALLOCATED: AtomicU64 = AtomicU64::new(0);

fn record(size: usize) {
    LARGEST_ALLOC.fetch_max(size as u64, Ordering::Relaxed);
    TOTAL_ALLOCATED.fetch_add(size as u64, Ordering::Relaxed);
}

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Every F03-B refusal is arithmetic: none of them builds the buffer its
/// input claims to describe.
#[test]
fn accept_f03_b_refusals_allocate_no_buffer_from_input_lengths() {
    // Setup outside the measured section: labels and the input slice are
    // allocated here, where the budget's counters are not being watched.
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    let recursion = RecursionBudget::new(CONTAINER, 1);
    let mut reader = Reader::new(CONTAINER, &[0u8; 8]);
    let claimed = u32::MAX as u64 * 8;

    LARGEST_ALLOC.store(0, Ordering::Relaxed);
    TOTAL_ALLOCATED.store(0, Ordering::Relaxed);

    // Measured section: refusals only, no formatting while counters run.
    let count_refusal = budget.reserve("table.count", 0, u32::MAX as u64, 8);
    let product_refusal = budget.reserve("table.count", 0, u64::MAX, 8);
    let extent_refusal = budget.reserve_extent("member.range", u64::MAX - 3, 8);
    let reader_extent = reader.checked_extent("member.range", u64::MAX - 3, 8);
    let reader_product = reader.checked_byte_len("table.count", u64::MAX, 8);
    let borrowed = reader.read_bytes("table.entries", u32::MAX as usize);
    let outer = recursion.enter("node", 0);
    let inner = recursion.enter("node", 1);

    // Read the counters before anything formats a message.
    let largest = LARGEST_ALLOC.load(Ordering::Relaxed);
    let total = TOTAL_ALLOCATED.load(Ordering::Relaxed);

    assert_eq!(
        count_refusal
            .expect_err("u32::MAX * 8 bytes exceeds the default budget")
            .kind,
        ParseErrorKind::AllocationBudgetExceeded
    );
    assert_eq!(
        product_refusal
            .expect_err("u64::MAX * 8 overflows the product")
            .kind,
        ParseErrorKind::LengthOverflow
    );
    assert_eq!(
        extent_refusal
            .expect_err("(u64::MAX - 3) + 8 overflows the extent")
            .kind,
        ParseErrorKind::LengthOverflow
    );
    assert_eq!(
        reader_extent
            .expect_err("the reader refuses the overflowing extent too")
            .kind,
        ParseErrorKind::LengthOverflow
    );
    assert_eq!(
        reader_product
            .expect_err("the reader refuses the overflowing product too")
            .kind,
        ParseErrorKind::LengthOverflow
    );
    assert_eq!(
        borrowed
            .expect_err("4 bytes cannot back a u32::MAX-length borrow")
            .kind,
        ParseErrorKind::UnexpectedEof
    );
    outer.expect("level 1 fits the one-level budget");
    assert_eq!(
        inner
            .expect_err("level 2 exceeds the one-level budget")
            .kind,
        ParseErrorKind::RecursionDepthExceeded
    );
    assert_eq!(reader.remaining(), 8, "no refusal consumed any input byte");

    assert!(
        largest < MAX_SINGLE_ALLOC,
        "a refusal allocated {largest} bytes in one block while the input claimed {claimed} bytes"
    );
    assert!(
        total < MAX_SINGLE_ALLOC,
        "the refusal path allocated {total} bytes in total; only bounded error strings are allowed"
    );
}
