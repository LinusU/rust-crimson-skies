# F03-B: the bounded allocation and recursion utilities, and what they do not claim

Date: 2026-09-28. Task: F03-B "Add bounded allocation and recursion utilities"
(`specs/F03-bounded-binary-parsing-primitives.md`). Capabilities used: ordinary
build/test only; `CS_GAME_DIR` was not read and no original data appears in
this change.

## What was built

| file | contents |
|---|---|
| `crates/cs_formats/src/io.rs` | `AllocationBudget { new, with_defaults, limit, used, remaining, reserve, reserve_extent }` and `RecursionBudget { new, with_defaults, max_depth, depth, enter }` plus `RecursionGuard { level }` (releases its level on drop) |
| `crates/cs_formats/src/error.rs` | `ParseErrorKind::AllocationBudgetExceeded` (`allocation_budget_exceeded`) and `ParseErrorKind::RecursionDepthExceeded` (`recursion_depth_exceeded`) with the matching constructors |
| `crates/cs_formats/src/lib.rs` | wiring only: re-exports `AllocationBudget`, `RecursionBudget`, `RecursionGuard` |
| `crates/cs_formats/tests/` | four new `accept_f03_b_*` integration-test binaries (13 tests) |

Both utilities are counters plus checked arithmetic over provenance-carrying
labels: they never touch bytes, never build a buffer and never widen a limit
behind the caller's back. Stage F03-C wires them into real parser entrypoints;
nothing here changes an existing parser.

## Design decisions (engineered, not observed)

No claim about any original file layout, tuning value or game rule is made or
implied by these numbers:

1. **Independent limits, one dimension each** (non-negotiable #2). Allocation
   and recursion have separate budgets with separate error kinds, so exhausting
   one cannot relax or hide the other. Entry counts, texture dimensions and
   decompressed-output limits belong to the stages that own those formats and
   are deliberately *not* folded in here.
2. **Designed defaults, explicit configuration.**
   `AllocationBudget::DEFAULT_LIMIT` = 64 MiB (a 4096² RGBA8 texture is
   exactly 64 MiB; a hostile `u32::MAX`-entry table is 32 GiB and is refused
   long before that). `RecursionBudget::DEFAULT_MAX_DEPTH` = 32. Any other
   value must be passed explicitly to `new`, and the tested configuration
   surface is pinned by tests at 0, at the exact fit and at the default.
3. **Charge only on success.** A refused reservation leaves `used` untouched,
   so a failing parse cannot drain the budget of a later, honest attempt.
   `reserve`/`reserve_extent` are the only writers of `used`.
4. **RAII depth accounting.** `enter` borrows `&self` (atomic increment via
   `fetch_update`, so a ceiling cannot be raced past) and returns a guard whose
   `Drop` releases exactly the level it took — on `?` error paths as well as
   success, which is what makes cyclic or over-deep member lists terminate
   (non-negotiable #3) instead of leaking depth.
5. **Errors carry metadata only**, like F03-A: container label, offset,
   field, expected and observed *counts*. Budget messages hold
   `available of limit allocation-budget bytes available` / `N bytes
   requested`; no payload byte is copied.
6. **`reserve_extent` reports the range's own start offset** in its error,
   because a budget has no read position; `Reader::checked_extent` keeps
   reporting the reader's current position. Both shapes are asserted.

## Sensitivity evidence

Four temporary mutations to `crates/cs_formats/src/io.rs`, each reverted
immediately afterwards (the working tree was restored from a copy and
`git diff` shows only the intended additions):

| mutation | result |
|---|---|
| `charge`: `let available = self.remaining()` → `let available = u64::MAX` | `accept_f03_b_allocation_budget` 4/4 **FAILED**, `accept_f03_b_u32_max_count_is_refused_by_the_default_budget` **FAILED**, `accept_f03_b_refusals_allocate_no_buffer_from_input_lengths` **FAILED** (exit 101) |
| `enter`: `if depth < self.max_depth` → `if true` | `accept_f03_b_recursion_budget` 3/3 **FAILED** (exit 101) |
| `reserve_extent`: `offset.checked_add(len)` → `Some(offset.wrapping_add(len))` | `accept_f03_b_offset_plus_length_overflow_is_refused_without_allocation` **FAILED** |
| `reserve`: `count.checked_mul(element_size)` → `Some(count.wrapping_mul(element_size))` | `accept_f03_b_overflowing_products_are_length_overflows` **FAILED** |

After restoring the original file all 13 tests pass again.

The "without allocation" half of AC02 is *measured*, not argued:
`accept_f03_b_no_large_allocation.rs` installs a counting `#[global_allocator]`
in its own test binary (one test per binary, so no sibling thread can move the
counters) and asserts that no single block of 64 KiB or more is allocated while
every F03-B refusal runs. Error `String`s are a few dozen bytes and are bounded
by fixed labels; a buffer sized from the input would be 32 GiB and trip the
assertion immediately.

## Commands run (all exit 0 unless noted)

| command | exit |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f03_b_ --include-ignored` | 0 (13 tests) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f03_b_` | 0 (13 selected, 13 re-ran `--exact`) |
| the four mutation probes above | 101 / FAILED (expected) |

## Open point for a later stage (recorded, not guessed)

The budgets are primitives with no consumer yet: no parser calls `reserve` or
`enter`, so *retail* behaviour is unaffected until F03-C wires them into the
actual entrypoints and F03-D runs the truncation/fuzz corpus against them. The
defaults above are **designed** engineering budgets (EvidenceClass
`Designed` in `docs/contracts/IDENTITY-CONTENT.md`), not measured original
values, and no original-data claim is made for them.

## Review additions to this stage (same task, reviewer pass)

Three corrections were made while reviewing the branch; none changes a limit,
an error kind or a tested boundary:

1. **Two doc statements were made truthful.** The `AllocationBudget` type
   documentation claimed that cloning "starts fresh accounting", which
   contradicted both the code (a clone carries `limit` and `used`) and
   `accept_f03_b_budgets_are_independent`; it now says a clone is an
   independent ledger carrying the same `limit` and `used` — never a shared
   counter and never a wider allowance. The `RecursionBudget::DEFAULT_MAX_DEPTH`
   documentation said the depth was "observed in the engine's own content
   model"; nothing has been observed — no original data was read — so both
   default docs now say plainly that these are *designed* budgets
   (EvidenceClass `Designed`) and no original value is implied. This removes
   an implied `Observed`/`VerifiedOriginal` claim, per
   `docs/contracts/IDENTITY-CONTENT.md`.
2. **Cross-dimension independence is now pinned by a test.**
   `accept_f03_b_allocation_and_recursion_limits_are_independent` exhausts an
   `AllocationBudget` and then shows the `RecursionBudget` still enforces its
   own ceiling, and that entering — or being refused at — that ceiling charges
   no allocation bytes (spec non-negotiable #2 applies *across* dimensions,
   not only between two budgets of the same kind). Task tests: 12 → 13.
3. **Sensitivity was re-measured after the addition**, with the working copy
   restored byte-for-byte afterwards (`cmp` clean):

   | mutation (both reverted) | result |
   |---|---|
   | `charge`: `if bytes > available` → `if false` | the 5 allocation tests, `accept_f03_b_refusals_allocate_no_buffer_from_input_lengths` and `accept_f03_b_u32_max_count_is_refused_by_the_default_budget` **FAILED** |
   | `enter`: `if depth < self.max_depth` → `if true` | the 3 recursion tests, the no-allocation test, the u32::MAX test **FAILED**, and the new cross-dimension test **FAILED** |

   Gates re-run after these edits: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets --all-features --locked -- -D
   warnings`, `cargo test --workspace --locked`, the `accept_f03_b_`
   selection (13/13) and `cs_xtask test-select --prefix accept_f03_b_`
   (13 selected, 13 re-ran `--exact`), all exit 0.
