# F03-C: the contextual-error parser entrypoint, and what it does not claim

Date: 2026-09-28. Task: F03-C "Integrate contextual errors into all parser
entrypoints" (`specs/F03-bounded-binary-parsing-primitives.md`). Capabilities
used: ordinary build/test only; `CS_GAME_DIR` was not read and no original
data appears in this change.

## What was built

| file | contents |
|---|---|
| `crates/cs_formats/src/io.rs` | `ParseContext { new, with_defaults, container, allocation, recursion, parse }` — the entrypoint that hands one attempt a reader, the allocation budget and the recursion budget under a single provenance label, rolls a failed attempt's charges back and scopes the error as it propagates out — plus the private `AllocationBudget::rollback_to` it uses |
| `crates/cs_formats/src/error.rs` | `ParseError::in_scope(scope)`: prefixes the logical field path with the name of the entrypoint the error crossed, leaving container, absolute offset, kind and expected/observed untouched |
| `crates/cs_formats/src/lib.rs` | wiring only: re-export of `ParseContext`, doc comment naming F03-C |
| `crates/cs_formats/tests/` | four new `accept_f03_c_*` integration-test binaries (9 tests: three binaries / 8 tests from the implementer, one binary / 1 test added in review) |

Nothing in F03-A or F03-B changed behaviour: `Reader` operations and budget
operations already produced contextual errors; this stage is the integration
point that runs them together.

## Design decisions (engineered, not observed)

No claim about any original file layout, tuning value or game rule is made or
implied by any of this (EvidenceClass `Designed` in
`docs/contracts/IDENTITY-CONTENT.md`):

1. **One container label per parse.** `ParseContext` owns the label and builds
   the reader *and* both budgets from it, so a refusal raised by
   `AllocationBudget::reserve` inside an attempt names the same archive as one
   raised by `Reader::read_u32`. Before this stage the two were independent
   strings a caller had to keep in step.
2. **Three disjoint references per attempt.** `ParseContext::parse` hands its
   closure `&mut Reader`, `&mut AllocationBudget` and `&RecursionBudget`
   as *separate* parameters rather than a single `&mut ParseContext`. A parser
   that holds a `RecursionGuard` while reserving the bytes of the level it
   descended into is therefore legal — with one `&mut self` accessor surface
   the guard's borrow would block the reservation (and non-negotiable #2/#3
   need both at once). Nothing is charged for entering an attempt.
3. **Teardown: a failed attempt leaves no trace.** Its guards and buffers are
   the closure's locals and are dropped when it returns, so the recursion
   depth returns to its entry value, and `parse` rolls the allocation ledger
   back to the mark it took on entry. This is what makes *retry* honest: 100
   failed attempts that each took a whole budget do not drain it for the 101st
   (`accept_f03_c_hostile_attempts_never_drain_the_budget`).
4. **`used` now has three writers.** F03-B's finding recorded
   `reserve`/`reserve_extent` as the only writers of `used`; F03-C adds
   `rollback_to`, which is private, called only by `ParseContext::parse`, and
   can only move the ledger *backwards* to an earlier state of the same
   budget. `used <= limit` still holds, no allowance is ever widened, a
   refused reservation is still never charged, and a *successful* attempt is
   still never rolled back (`accept_f03_c_successful_attempts_keep_their_charges`).
   The rollback assumes a failed attempt released what it reserved, which is
   what Rust's ownership gives for the attempt's own locals.
5. **Every entrypoint names itself on the error path.** `ParseContext::parse`
   applies `ParseError::in_scope(entrypoint)`, and a nested parser entrypoint
   applies it too, so field paths read outside in:
   `record.entries[3].header.count`. Scoping is purely additive: container,
   absolute offset, kind and the expected/observed strings are byte-identical
   to what the failing operation produced. An empty scope adds nothing rather
   than a stray separator.
6. **Unaligned input stays unaligned-input (AC03).** The entrypoint only
   constructs a byte-wise reader; it never copies or re-aligns the slice, so
   the deliberately odd-addressed record parses to the same typed values
   through it, and truncating it at every one of the 23 byte boundaries still
   yields a scoped `UnexpectedEof` naming the field, the absolute offset and
   the expected/observed byte counts — never a panic.
7. **The attempt's reader is tied to its bytes (added in review).** `parse`
   takes `bytes: &'bytes [u8]` and hands the closure `&mut Reader<'bytes>`,
   so an attempt can *return* a bounded field as a slice of the input
   (`Reader::read_bytes`, `read_str`, `read_bounded_cstr`, `sub_reader`)
   instead of a heap copy of it. The signature as first submitted used a
   higher-ranked `&mut Reader<'_>` with a fixed return type `T`, which cannot
   name the reader's lifetime: a probe returning `reader.read_str(...)`
   straight out of the entrypoint failed to compile with
   `lifetime may not live long enough` (exit 101). That would have forced
   every consumer of the entrypoint (F05+, F06+, F12+) to copy each bounded
   field onto the heap just to return it — an allocation that by-passes
   `AllocationBudget` and undercuts non-negotiable #2. Nothing that compiled
   before stops compiling: the reader is covariant in its lifetime, so
   callers passing a shorter borrow still work.

## Sensitivity evidence

Four temporary mutations from implementation plus one from review, each
applied to `crates/cs_formats/src/io.rs` and reverted immediately afterwards
(verified byte-for-byte against a saved copy with `cmp` after each probe).
The four implementation probes ran against the 8 tests that existed then; the
review probe ran against all 9:

| mutation | result (`cargo test -p cs_formats --locked --no-fail-fast -- accept_f03_c_`) |
|---|---|
| `parse`: drop only `Err(error.in_scope(entrypoint))` → `Err(error)` | exit **101**; **6 of 8** FAILED (only the two all-success tests pass) |
| `parse`: drop only `self.allocation.rollback_to(mark)` | exit **101**; **3 of 8** FAILED: budget provenance, teardown/retry, hostile attempts |
| `Reader::take`: `if available < len` → `if false && available < len` | exit **101**; **4 of 8** FAILED, including the unaligned truncation sweep |
| `parse`: `Reader::new(self.container.clone(), bytes)` → `Reader::new("", bytes)` | exit **101**; **3 of 8** FAILED (container assertions) |
| **review**: revert `parse` to `parse<T>` with `&mut Reader<'_>` (decision 7) | exit **101**; `accept_f03_c_entrypoint_hands_back_borrowed_input` fails to compile with `lifetime may not live long enough`, **0 of 9** tests run |

The first two probes were re-run during review and reproduced exactly (6/8
and 3/8 failed, exit 101 each) with the file restored byte-identically
afterwards.

The "one observable failure" named before editing holds: without the
`in_scope` call in `parse`,
`accept_f03_c_entrypoint_failures_keep_context_and_scope` fails with
`header.count` where it asserts `record.entries[3].header.count`.

## Commands run (all exit 0 unless noted)

| command | exit |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (72 passing test-result units, including the new `ParseContext` doctest) |
| `cargo test --workspace --locked -- accept_f03_c_ --include-ignored` | 0 (**8** tests, all passing) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f03_c_` | 0 (8 selected, 8 re-ran `--exact`) |
| the four mutation probes above | 101 / FAILED (expected) |
| **review** `cargo fmt --all -- --check` | 0 |
| **review** `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| **review** `cargo test --workspace --locked` | 0 (73 test-result units: 172 passed, 0 failed, 7 ignored) |
| **review** `cargo test --workspace --locked -- accept_f03_c_ --include-ignored` | 0 (**9** tests, all passing) |
| **review** the two re-run mutation probes and the signature probe | 101 / FAILED (expected) |

## Open point for a later stage (recorded, not guessed)

`cs_formats` still contains no format-specific parser: F05 (`rof`), F06
(`zbd`), F07 (`interp`), F08 (`texture`) and the rest are unimplemented
tasks, so `ParseContext::parse` has no *retail* consumer yet — it is the
entrypoint those stages must call instead of `Reader::new` when they land, and
retail behaviour is unaffected until then. The synthetic record in
`crates/cs_formats/tests/common/mod.rs` is the consumer used here; it is newly
authored bytes, not original data. Whether any retail container's strings are
NUL-terminated, length-prefixed or fixed-padded remains **unknown** and stays
with the per-format stages, exactly as `docs/findings/2026-09-23-f03-a-*.md`
recorded. No installation hash is attached to `ParseError`: converting it into
a `SourceSpan` still belongs to the consumer stage that has the install hash.
