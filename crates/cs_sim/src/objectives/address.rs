//! The cross-objective address rule (task `M02-B-FU3`, Rally #802): what this
//! engine does when a directive names an objective **past the record's block
//! count**.
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("IR requirements":
//! *"Validate names/ids/ranges"*). The measurement behind everything below is
//! static code evidence from the owner's decrypted executable, recorded with
//! its residual unknowns in
//! `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`; no
//! original program was run, so nothing here is `verified_original` behaviour.
//!
//! # What an address is
//!
//! An objective record's directive sites spell integers to name *other
//! objectives of the same record* — `WAKE_OBJECTIVE` /
//! `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (`+0x1c`), `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`
//! (`+0x58`), `KILL_OBJECTIVE_WHEN_I_COMPLETE` (`+0x94`),
//! `NAP_OBJECTIVE_WHEN_I_COMPLETE` (`+0xd0`), `WAKE_OBJECTIVE_WHEN_I_SLEEP`
//! (`+0xdc`), `TICK_DEPENDS_ON_OBJ` (`+0x10`) and `HIDE_OBJ` (`+0xd8`). Those
//! integers are the addresses this module resolves.
//!
//! # The measurement the rule rests on
//!
//! Read out of `crimson.decrypted.exe` (PE32, image base `0x400000`, SHA-256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`):
//!
//! * **The parse decrements the address.** The `WAKE_OBJECTIVE` /
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE` store loop (`0x468c40`) loads each child
//!   payload and executes `dec` before writing it into the `−1`-terminated
//!   `+0x1c` array; `NAP`'s target (`0x468cf0`) and `TICK_DEPENDS_ON_OBJ`
//!   (`0x4679fc`) do the same. The decrement is **specific to objective
//!   addresses**: `DEDG`'s two integers are stored without it (`0x467a21` →
//!   `+0x580`), so it is not an encoding shared by every payload. A spelled
//!   address `a` therefore becomes record index `a − 1` — the authored number
//!   is **one-based**, which is consistent with F39-E2's closure over all 1706
//!   retail branch-effect targets (every one names a declared `OBJECTIVE<n>`
//!   number, so none of them spells the `0` a zero-based reading would need to
//!   reach a record's first block); the parse's `dec` is what decides it.
//! * **The record holds one 0x5e4-byte record per numbered block**, appended in
//!   document order (`0x467956` reallocates by `0x5e4` per block), and the
//!   count field `+0xc48` is the block count (`0x469043` writes the block
//!   loop's exit counter).
//! * **The wake walk `0x469af0` has no address check at all**: it computes
//!   `records + 0x5e4 × stored_index` and touches that record — no comparison
//!   against `+0xc48`, no clamp, no ignore, no log. Two sibling accessors
//!   (`0x469800`, `0x469860`) *do* test the address, but reject only
//!   `address > count`, so they accept `address == count` — one record past the
//!   array.
//!
//! # The named engine rule
//!
//! > **`OUT_OF_RANGE_OBJECTIVE_ADDRESS`**: a cross-objective address resolves
//! > only inside `[1, objectives]` (one-based, matching the parse's `dec`) and
//! > maps to [`SymbolId`] `address − 1`. An address outside that range is
//! > **refused by name** — never clamped to the nearest live objective, never
//! > ignored, never silently accepted.
//!
//! Refusal is the safe reading of an *unmeasured* original behaviour: the
//! original's unchecked walk touches whatever memory follows the array, so what
//! a player would observe there depends on the allocator's neighbours and is
//! recorded as an unknown rather than guessed (AGENTS.md rule 4). Clamping
//! would invent a target the record did not spell, and accepting silently would
//! hide a data error behind a directive that appears to work.

use std::fmt;

use cs_script::ir::SymbolId;

/// The rule's name, as every diagnostic this module produces spells it.
pub const OUT_OF_RANGE_OBJECTIVE_ADDRESS: &str = "OUT_OF_RANGE_OBJECTIVE_ADDRESS";

/// Why a cross-objective address could not be resolved.
///
/// Carries both numbers so a report can say *which* address was past *which*
/// record, instead of a bare failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressRefusal {
    /// The address exactly as it was spelled.
    pub address: i64,
    /// How many objectives the record declares — the one-based upper bound.
    pub objectives: u32,
}

impl fmt::Display for AddressRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{OUT_OF_RANGE_OBJECTIVE_ADDRESS}: cross-objective address {} is outside the \
             record's {} objective(s); addresses are one-based and out-of-range addresses are \
             refused, never clamped",
            self.address, self.objectives
        )
    }
}

impl std::error::Error for AddressRefusal {}

/// Resolves one spelled cross-objective address against the record's objective
/// count under [`OUT_OF_RANGE_OBJECTIVE_ADDRESS`].
///
/// `objectives` is the record's block count — the same count the original keeps
/// at `+0xc48`. In range means `1 <= address <= objectives`, because the
/// original's parse stores `address − 1` and its record array holds exactly
/// `objectives` records at indices `0..objectives`.
///
/// # Errors
///
/// [`AddressRefusal`] for every address outside `[1, objectives]`, including
/// every address a record with no objectives receives. The refusal is the
/// result: no clamped symbol, no default, no silently accepted address.
pub fn resolve_objective_address(
    address: i64,
    objectives: u32,
) -> Result<SymbolId, AddressRefusal> {
    let refusal = AddressRefusal {
        address,
        objectives,
    };
    if address < 1 || address > i64::from(objectives) {
        return Err(refusal);
    }
    // `address >= 1` and `address <= objectives: u32`, so the decrement is
    // exactly the record index and fits `u32`.
    Ok(SymbolId((address - 1) as u32))
}

/// The one-based address a symbol is reached by — the inverse of
/// [`resolve_objective_address`], so a caller that holds a symbol can spell the
/// address the record's own directives use.
#[must_use]
pub fn address_of(symbol: SymbolId) -> i64 {
    i64::from(symbol.0) + 1
}
