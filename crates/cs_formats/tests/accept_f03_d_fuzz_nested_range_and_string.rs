//! Acceptance scenario F03-D (AC04): fuzz nested range and string decoding
//! with deterministic seeds.
//!
//! The corpus is fixed — a SplitMix64 stream per seed, a set of emitted valid
//! buffers and handcrafted hostile vectors — so every failure names one
//! reproducible input (case name plus seed). Every case runs through the
//! production entrypoint `ParseContext::parse` with the resource limits
//! recorded in `docs/findings/2026-09-28-f03-d-*`:
//!
//! * `accept_f03_d_fuzz_corpus_decodes_without_panicking_inside_recorded_limits`
//!   asserts the invariants that must hold whatever the bytes say: no panic,
//!   an error that names its container, stays inside the input and keeps the
//!   entrypoint scope, budgets and recursion depth back at zero afterwards,
//!   and bounded fields that point into the input rather than into a copy;
//! * `accept_f03_d_fuzz_corpus_covers_every_kind_and_every_refusal` asserts
//!   the corpus is not vacuous: every node variant, every reachable refusal
//!   class, both successes and failures, and peaks that never cross the
//!   recorded limits;
//! * `accept_f03_d_hostile_vectors_report_their_recorded_refusal` pins each
//!   handcrafted input to the refusal the recorded limits promise, including
//!   the ledger a failed attempt leaves behind.
//!
//! Raw random bytes are newly authored synthetic data: no `CS_GAME_DIR` read,
//! nothing derived from the original installation.

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;

use common::nested::{
    CORPUS_CONTAINER, EMIT_SEEDS, FUZZ_RANDOM_SEEDS, RECORDED_ALLOCATION_LIMIT, RECORDED_MAX_DEPTH,
    assert_slices_within, crafted_cases, decode_all, emit_case, random_case, recorded_context,
};

/// Every case of the recorded fuzz corpus, in a fixed order.
fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    for case in crafted_cases() {
        cases.push((case.name.to_owned(), case.bytes));
    }
    for seed in 0..EMIT_SEEDS {
        cases.push((format!("emitted seed {seed}"), emit_case(seed)));
    }
    for seed in 0..FUZZ_RANDOM_SEEDS {
        cases.push((format!("random seed {seed}"), random_case(seed)));
    }
    cases
}

/// Invariants per case: whatever the bytes say, a decode either succeeds with
/// slices that point into the input, or fails with a contextual error and
/// leaves both budgets exactly as it found them.
#[test]
fn accept_f03_d_fuzz_corpus_decodes_without_panicking_inside_recorded_limits() {
    let mut successes = 0;
    let mut refusals = 0;

    for (name, bytes) in corpus() {
        let peak_depth = Cell::new(0u32);
        let mut context = recorded_context();

        match decode_all(&mut context, &bytes, &peak_depth) {
            Ok(stats) => {
                successes += 1;
                assert!(
                    stats.nodes > 0,
                    "{name}: a successful decode does no work at all"
                );
                assert_slices_within(&name, &bytes, &stats.slices);
                assert!(
                    context.allocation().used() <= RECORDED_ALLOCATION_LIMIT,
                    "{name}: charged {} of {} bytes after success",
                    context.allocation().used(),
                    RECORDED_ALLOCATION_LIMIT,
                );
            }
            Err(error) => {
                refusals += 1;
                assert_eq!(
                    error.container, CORPUS_CONTAINER,
                    "{name}: the error lost its provenance"
                );
                assert!(
                    error.offset <= bytes.len() as u64,
                    "{name}: offset {} leaves the {}-byte input",
                    error.offset,
                    bytes.len(),
                );
                assert!(
                    error.field.starts_with("record."),
                    "{name}: field `{}` lost the entrypoint scope",
                    error.field,
                );
                assert!(
                    !error.expected.is_empty() && !error.observed.is_empty(),
                    "{name}: error dropped its expected/observed conditions: {error}"
                );
                assert_eq!(
                    context.allocation().used(),
                    0,
                    "{name}: a failed attempt must book nothing ({error})"
                );
            }
        }

        assert_eq!(
            context.recursion().depth(),
            0,
            "{name}: every recursion guard was released"
        );
        assert!(
            peak_depth.get() <= RECORDED_MAX_DEPTH,
            "{name}: entered depth {} past the recorded limit {RECORDED_MAX_DEPTH}",
            peak_depth.get(),
        );
    }

    assert!(
        successes > 0 && refusals > 0,
        "the corpus must contain both accepted and refused inputs: \
         {successes} accepted, {refusals} refused"
    );
}

/// The corpus must not be vacuous: it has to actually reach every node
/// variant, every refusal class the grammar can produce, and the peaks of
/// both recorded limits.
#[test]
fn accept_f03_d_fuzz_corpus_covers_every_kind_and_every_refusal() {
    let mut cases = 0;
    let mut successes = 0;
    let mut refusals = 0;
    let mut nodes = 0usize;
    let mut variants = [0usize; 4];
    let mut bounded_fields = 0usize;
    let mut error_kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut peak_depth = 0u32;
    let mut max_used = 0u64;

    for (_, bytes) in corpus() {
        cases += 1;
        let depth = Cell::new(0u32);
        let mut context = recorded_context();

        match decode_all(&mut context, &bytes, &depth) {
            Ok(stats) => {
                successes += 1;
                nodes += stats.nodes;
                for (index, count) in stats.kinds.iter().enumerate() {
                    variants[index] += count;
                }
                bounded_fields += stats.slices.len();
                max_used = max_used.max(context.allocation().used());
            }
            Err(error) => {
                refusals += 1;
                *error_kinds.entry(error.kind.as_str()).or_insert(0) += 1;
            }
        }
        peak_depth = peak_depth.max(depth.get());
    }

    let expected_cases = crafted_cases().len() + EMIT_SEEDS as usize + FUZZ_RANDOM_SEEDS as usize;
    assert_eq!(cases, expected_cases, "the recorded corpus size changed");
    assert!(
        successes > 0 && refusals > 0,
        "both outcomes must occur: {successes} accepted, {refusals} refused"
    );
    assert!(nodes > 0 && bounded_fields > 0, "no node was decoded");

    for (variant, count) in variants.iter().enumerate() {
        assert!(
            *count > 0,
            "the corpus never decoded node variant {variant}: {variants:?}"
        );
    }

    // The grammar's five reachable refusal classes: the sixth,
    // `length_overflow`, needs a `count * element_size` product that exceeds
    // `u64`/`usize`, which a `u32` count times a `u8` element size cannot
    // produce — that path is pinned by the F03-B tests instead.
    for kind in [
        "unexpected_eof",
        "invalid_encoding",
        "missing_terminator",
        "allocation_budget_exceeded",
        "recursion_depth_exceeded",
    ] {
        assert!(
            error_kinds.contains_key(kind),
            "the corpus never produced {kind}: {error_kinds:?}"
        );
    }

    assert_eq!(
        peak_depth, RECORDED_MAX_DEPTH,
        "the deep chain must reach the recorded ceiling exactly, and no case \
         may pass it"
    );
    assert_eq!(
        max_used, RECORDED_ALLOCATION_LIMIT,
        "the exact-fit table must charge the recorded limit in full, and no \
         case may charge past it"
    );

    println!(
        "f03-d fuzz: {cases} cases ({successes} accepted, {refusals} refused), \
         {nodes} nodes, variants {variants:?}, {bounded_fields} bounded fields, \
         error kinds {error_kinds:?}, peak depth {peak_depth}/{RECORDED_MAX_DEPTH}, \
         peak allocation {max_used}/{RECORDED_ALLOCATION_LIMIT} bytes"
    );
}

/// Each handcrafted input pinned to the refusal its recorded limits promise,
/// and to the allocation ledger it must leave behind.
#[test]
fn accept_f03_d_hostile_vectors_report_their_recorded_refusal() {
    for case in crafted_cases() {
        let name = case.name;
        let peak_depth = Cell::new(0u32);
        let mut context = recorded_context();
        let outcome = decode_all(&mut context, &case.bytes, &peak_depth);

        match (case.expect, outcome) {
            (Some(expected), Err(error)) => {
                assert_eq!(error.kind, expected, "{name}: {error}");
                assert_eq!(error.container, CORPUS_CONTAINER, "{name}");
                assert!(
                    error.offset <= case.bytes.len() as u64,
                    "{name}: offset {} leaves the {}-byte input",
                    error.offset,
                    case.bytes.len(),
                );
            }
            (None, Ok(stats)) => {
                assert!(stats.nodes > 0, "{name}: parsed nothing");
                assert_slices_within(name, &case.bytes, &stats.slices);
            }
            (expected, outcome) => panic!(
                "{name}: expected {expected:?}, got {}",
                match outcome {
                    Ok(stats) => format!("{} nodes parsed", stats.nodes),
                    Err(error) => format!("{error}"),
                },
            ),
        }

        assert_eq!(
            context.allocation().used(),
            case.expect_used,
            "{name}: allocation ledger after the attempt"
        );
        assert_eq!(
            context.recursion().depth(),
            0,
            "{name}: recursion depth after the attempt"
        );
        assert!(
            peak_depth.get() <= RECORDED_MAX_DEPTH,
            "{name}: entered depth {} past the recorded limit",
            peak_depth.get(),
        );
    }
}
