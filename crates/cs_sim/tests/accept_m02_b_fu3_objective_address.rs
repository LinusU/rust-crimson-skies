//! `M02-B-FU3` acceptance: the cross-objective address rule (synthetic — every
//! case runs in CI with no original data).
//!
//! Task: Rally #802, "Measure what the original does with a cross-objective
//! address past the record's block count". Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("IR requirements"). Measurement and its
//! residual unknowns: `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`.
//!
//! The rule under test is
//! [`OUT_OF_RANGE_OBJECTIVE_ADDRESS`](cs_sim::objectives::address):
//!
//! * a spelled address is **one-based** — the original's parse stores
//!   `address − 1`, so an address inside `[1, objectives]` resolves to the
//!   record index `address − 1`;
//! * an address **past the record's block count is refused by name**: never
//!   clamped to the last record, never ignored, never silently accepted.
//!
//! The retail half of the same rule — that every address M02's own record
//! spells, including the `50` beside `OBJECTIVE13`, resolves inside its
//! 50-block record — is
//! `accept_m02_b_fu3_m02s_wake_addresses_resolve_inside_its_block_count` in the
//! campaign suite.

use cs_script::ir::SymbolId;
use cs_sim::objectives::address::{
    AddressRefusal, OUT_OF_RANGE_OBJECTIVE_ADDRESS, address_of, resolve_objective_address,
};
use cs_sim::objectives::runtime::RuntimeError;

/// M02's own numbers, as M02-B's graph test pins them from the decoded
/// document: a record that declares 50 blocks, and the address `50` block
/// `OBJECTIVE13` wakes — the address that sits exactly on the count, which a
/// zero-based reading would put past the record's end and the measured
/// one-based reading spells as the last block.
const M02_BOUNDARY_ADDRESS: i64 = 50;
const M02_BLOCKS: u32 = 50;

/// **An address inside the record resolves to its one-based record index.**
///
/// The one-based reading is the measured one: the original's parse decrements
/// every objective address before storing it (`0x468c40`, `0x468cf0`,
/// `0x4679fc`), and its record array holds exactly one 0x5e4-byte record per
/// numbered block, so address `a` lands on record `a − 1`.
#[test]
fn accept_m02_b_fu3_an_address_inside_the_record_resolves_to_its_one_based_record_index() {
    // The first block and the last block are both reachable, and the mapping is
    // exactly `address − 1` — an off-by-one in either direction breaks one of
    // the two boundaries.
    assert_eq!(
        resolve_objective_address(1, M02_BLOCKS),
        Ok(SymbolId(0)),
        "address 1 names the first record"
    );
    assert_eq!(
        resolve_objective_address(M02_BOUNDARY_ADDRESS, M02_BLOCKS),
        Ok(SymbolId(49)),
        "M02's last spelled address names the last record of its 50, not a \
         record past the end"
    );
    assert_eq!(
        resolve_objective_address(7, 10),
        Ok(SymbolId(6)),
        "an address strictly inside a smaller record resolves the same way"
    );
    assert_eq!(
        resolve_objective_address(1, 1),
        Ok(SymbolId(0)),
        "a one-block record has exactly one reachable address"
    );

    // The inverse is exact: spelling the resolved address back gives the
    // address the record's own directives use.
    for address in [1, 2, 14, 50] {
        let symbol = resolve_objective_address(address, M02_BLOCKS)
            .unwrap_or_else(|refusal| panic!("address {address} is inside the record: {refusal}"));
        assert_eq!(
            address_of(symbol),
            address,
            "address {address} round-trips through the record index {symbol:?}"
        );
    }
}

/// **An address past the block count is refused by name — never clamped,
/// never ignored, never silently accepted.**
///
/// The original's wake walk (`0x469af0`) performs no address check at all: it
/// multiplies the stored index by the 0x5e4 record stride and touches that
/// record, so what a player would observe depends on the memory that follows
/// the array and stays **unknown** (findings §"Residual unknowns"). This engine
/// refuses instead, and this test is what keeps the refusal from quietly
/// becoming a clamp to the last record, a skip, or an `Ok`.
#[test]
fn accept_m02_b_fu3_an_address_past_the_block_count_is_refused_never_clamped() {
    // One past the count: the shape M02's document-level pin is about.
    let past = resolve_objective_address(M02_BOUNDARY_ADDRESS + 1, M02_BLOCKS)
        .expect_err("an address past the record's block count must refuse");
    assert_eq!(
        past,
        AddressRefusal {
            address: 51,
            objectives: M02_BLOCKS
        },
        "the refusal carries the address exactly as spelled, not a clamped one"
    );

    // Far past, zero and negative: every address outside [1, objectives]
    // refuses, and the refusal is the whole result — there is no symbol beside
    // it that a caller could use instead.
    for (address, objectives) in [
        (51, 50),
        (500, 50),
        (0, 50),
        (-1, 50),
        (i64::MIN, 50),
        (1, 0),
    ] {
        let refusal = resolve_objective_address(address, objectives)
            .expect_err("an address outside [1, objectives] must refuse");
        assert_eq!(
            refusal,
            AddressRefusal {
                address,
                objectives
            },
            "the refusal names address {address} against {objectives} objective(s)"
        );
        assert!(
            !(1..=i64::from(objectives)).contains(&address),
            "the test case {address} really is outside [1, {objectives}]"
        );
    }

    // A refused address is never resolved to a live record: the clamp the rule
    // forbids would answer with `SymbolId(49)` for 51 — which is exactly what
    // this asserts does *not* happen.
    assert!(
        resolve_objective_address(M02_BOUNDARY_ADDRESS + 1, M02_BLOCKS).is_err(),
        "address 51 never resolves to the last record"
    );

    // The refusal names the rule in the runtime's own error vocabulary, so the
    // objective lifecycle can report it instead of dropping it.
    let named: RuntimeError = past.into();
    let message = named.to_string();
    assert!(
        message.contains(OUT_OF_RANGE_OBJECTIVE_ADDRESS),
        "the runtime error names the rule: {message}"
    );
    assert!(
        message.contains("refused") && message.contains("never clamped"),
        "the diagnostic states what happens to an out-of-range address: {message}"
    );
    assert!(
        message.contains("51") && message.contains("50"),
        "the diagnostic carries the address and the record's block count: {message}"
    );
}
