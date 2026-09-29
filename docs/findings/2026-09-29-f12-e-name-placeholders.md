# F12-E: expanding `<NAME>` placeholders through a scoped name table

Date: 2026-09-29. Task: #370 "Expand `<NAME>` placeholders in configuration
values through a scoped name table" (key `F12-E`). Shared contract:
[`docs/contracts/IDENTITY-CONTENT.md`](../contracts/IDENTITY-CONTENT.md).
Required capability: ordinary build/test. This machine also has `retail`, and
it was used **read-only**: `ASSETS/LAYOUT.CSV` and `ASSETS/SCRAPBOOK.CSV` are
decoded out of `GOSDATA/ASSETS/crimson.rof` into `private/` (git-ignored) or
read straight from the archive by an `#[ignore = "requires CS_GAME_DIR"]`
test. Nothing from the installation is committed except paths, counts and
hashes. The findings that specify the rule are task #351's
([`2026-09-29-t351-keyed-list-reading-rules.md`](2026-09-29-t351-keyed-list-reading-rules.md),
section "Measured and recorded, but not implemented"); this stage implements
that rule as a second pass and changes none of it.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/text/placeholder.rs` (new): the pass
  `read_placeholders` / `scan_placeholders`, the owned `PlaceholderTable`,
  `PlaceholderDefinition`, `PlaceholderReference`, `ResolvedPlaceholder`,
  `PlaceholderScope`, the accounting and the two lexical predicates
  `definition_scope` and `placeholder_name`.
- `crates/cs_formats/src/text/tests_f12_e.rs` (new): the `accept_f12_e_*`
  tests.
- `crates/cs_formats/src/text/mod.rs`: module registration, re-exports and a
  doc bullet (wiring only).
- `crates/cs_formats/src/text/dialect.rs`: the `KeyedList` inventory row's
  `unknowns` no longer lump the `<NAME>` rule in with field types and units;
  the three placeholder unknowns it leaves are now spelled out.
- `crates/cs_content/src/config.rs`: `ConfigDocument::read` runs the pass and
  keeps the owned table; `ConfigDocument::placeholders` exposes it; a module
  doc paragraph; one `accept_f12_e_*` test.
- `docs/findings/2026-09-29-f12-e-name-placeholders.md` (this file) and
  `docs/findings/evidence/F12-E.json`.

**One observable failure:** the owned nodes are text, so a consumer that
reads a tuning field gets the bytes `<WIDTH>` and cannot turn them into `640`.
The raw bytes are data and must stay (`KeyedList::reassemble` returns the
member byte for byte), so the resolution has to be a second pass over the same
list — one that knows which definition is in scope and books its tables
against the parse budget. The pass now does that; the raw entries and the
`TuningSchema` conversion path are untouched (spec F12 non-negotiables #2 and
#5).

## What is established and implemented

The rule is task #351's; the pass adds no measurement of its own. It was
re-derived on this branch from the same two retail members to pin the
`accept_f12_e_retail_layout_references_all_resolve` test:

| Measurement | `ASSETS/LAYOUT.CSV` | `ASSETS/SCRAPBOOK.CSV` |
| --- | --- | --- |
| `V<digits>` / `G<digits>` definitions (value exactly two fields) | 186 (157 local + 29 global) | 0 |
| Whole-field `<NAME>` references | 1313 | 0 |
| References answered by a section-local definition | 793 | 0 |
| References answered by a global definition | 520 | 0 |
| Unresolved references | **0** | 0 |
| Distinct reference names | 113 | 0 |
| Local definitions shadowing a global name | 0 | 0 |
| Definition values that are themselves a `<NAME>` reference | 0 | 0 |
| Reference fields mixing `<NAME>` with other text | 0 | 0 |

Established and implemented as stated:

1. **A definition** is an entry whose key is `V<digits>` or `G<digits>` and
   whose value has exactly two fields `NAME,value`. `V` is section-local,
   `G` is global. `definition_scope` is the predicate.
2. **A reference** is a field whose value, compared as the reader hands it to
   a consumer (**R4**, blank bytes dropped), is exactly `<NAME>`:
   `placeholder_name` is the predicate. No surveyed field embeds a placeholder
   in larger text and no surveyed reference is quoted, so the pass does
   neither.
3. **Resolution order** is the local table of the reference's own section
   first, the global table otherwise. The section index is the line index of
   the header the entry follows, the same index `KeyedList::entries` yields.
4. **Raw bytes are kept.** The pass borrows the already-parsed `KeyedList`,
   writes only owned tables, and never edits a node:
   `KeyedList::reassemble` still returns the member byte for byte. A resolved
   value is bytes; converting it is still `TuningSchema`'s job against a
   declared `FieldSpec` (spec F12 #2).
5. **An unresolved name is reported, not guessed.** It is kept as a
   `PlaceholderReference` with `definition: None`, counted in
   `PlaceholderAccounting::unresolved`, and listed by
   `PlaceholderTable::unresolved`. Nothing substitutes a default or drops the
   field.
6. **The pass is bounded and budgeted.** Both tables and every byte they own
   are charged to the `ParseContext` allocation budget; the shapes are
   counted first so the vectors are reserved exactly; a refused reservation
   charges nothing (`ParseContext::parse` rolls the attempt back).

`ConfigDocument::read` calls the pass on the same list its owned nodes come
from, so the two can never disagree about a line, and
`ConfigDocument::placeholders` exposes the result. The pass is not a hidden
second parse: `read_placeholders` runs one `parse` attempt under the
`text.placeholders` entrypoint; `scan_placeholders` is the same body for a
caller that already holds a budget.

## What stays unknown and is not assumed

These are the same unknowns #351 recorded; the dialect inventory row now names
them individually and the code does not resolve any of them:

1. **Case.** Every observed name and reference is uppercase, so whether a
   placeholder name is compared with or without regard to ASCII case is
   unobserved. The pass compares exact bytes; a differently-cased name is
   reported unresolved rather than folded.
2. **Local-versus-global precedence.** No local definition shadows a global
   name in the surveyed member, so which table the original consults first
   is unobserved. The pass consults the local table first because the task
   states that reading; with no shadowing the choice cannot change a retail
   result.
3. **The expansion itself.** Whether the original substitutes the definition's
   value as text into the field, or binds it some other way (a typed
   assignment, a macro the interpreter evaluates), is not measured. The pass
   hands back the definition's value; a consumer must still declare the width,
   signedness and range of a field it turns into a number.
4. **Quoting and padding interactions**, and **chained definitions** (a
   definition whose value is itself a `<NAME>`), were never observed. A quoted
   field is not read as a reference; a padded but unquoted field is, because
   R4 already drops the padding. A definition value that is a placeholder is
   kept as bytes, not expanded.
5. **Bytes above 0x7F.** The code page of a localized installation is
   unmeasured, so a non-ASCII name is neither rejected nor transcoded; it is
   compared as bytes.

None of these is a placeholder to be filled in later by guessing: each would
need data that does not exist on this installation (a case-varied name, a
shadowing definition, a chained definition, a non-ASCII name, or the original
running).

## Mutation checks

Each `accept_f12_e_*` test was run against a deliberately broken build and
fails there; every probe was then reverted and the suite is green on the
restored code.

| Test | Mutation | Result |
| --- | --- | --- |
| `accept_f12_e_references_resolve_local_first_then_global` | drop the local-table lookup, resolve against globals only | fails (local answers change) |
| `accept_f12_e_unresolved_names_are_reported_not_guessed` | same mutation | fails (a local-only name becomes unresolved) |
| `accept_f12_e_retail_layout_references_all_resolve` | same mutation | fails (793/520 split changes) |
| `accept_f12_e_the_pass_is_bounded_by_the_allocation_budget` | remove every reservation (table shapes and per-byte copies) | fails (the pass books nothing) |
| `accept_f12_e_document_resolves_placeholders_and_keeps_the_raw_bytes` | replace the pass in `ConfigDocument::read` with an empty table | fails (accounting is empty) |

The budget probe had to remove **all** charging, not just the two table
reservations: with only the shape reservations gone the per-byte copies still
charge, and that is correct — the test asserts the pass books *something*
more than the list, and that the exact budget fits while one byte less is
refused after a rollback.

## Commands

```sh
# The task selection, including the ignored retail test (CS_GAME_DIR set):
cargo test --workspace --locked -- accept_f12_e_ --include-ignored

# Every check a push runs:
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
```

## Sources

- S02, S04 ([`docs/research/SOURCES.md`](../research/SOURCES.md)) as listed by
  the F12 feature sheet. No claim here rests on them: the rule and every
  number come from the two original members read read-only on this machine.
