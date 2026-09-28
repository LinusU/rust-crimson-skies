# F05-A: raw ROF structs and synthetic boundary fixtures

Date: 2026-09-28. Task: F05-A "Define raw ROF structs and synthetic boundary
fixtures" (`specs/F05-rof-directory-trees-and-compressed-members.md`,
section `### F05-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/rof.rs` (new): `DIRECTORY_ENTRYPOINT`,
  `DIRECTORY_HEADER_BYTES`, `RECORD_BYTES`, `FLAG_DIRECTORY`,
  `FLAG_COMPRESSED`, `KNOWN_FLAG_MASK`, `RofFlags` (`from_bits`, `bits`,
  `is_directory`, `is_compressed`, `unknown_bits`, `has_unknown_bits`),
  `RofRawHeader`, `RofRawRecord`, `RofDirectory` (`header`, `len`,
  `is_empty`, `records`, `record`, `name_bytes`, `entries`, `block_len`),
  `RofEntry`, `RofEntries`, `RofError` (`code`, `container`, `offset`,
  `From<ParseError>`, `Display`, `Error`), `read_directory`, and the
  private `decode_record` / `validate_names`.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod rof;`, the
  `pub use rof::{...}` re-exports and one module-doc sentence naming F05-A.
- `crates/cs_formats/tests/rof.rs` (new): the authored tree/block builders
  (`RawRecord`, `name_table`, `block`, `valid_block`, `tree`) and ten
  `accept_f05_a_*` tests.
- `docs/findings/2026-09-28-f05-a-raw-rof-structs-and-boundary-fixtures.md`
  (this file).

**Not created in this stage** (owner paths of later F05 stages, nothing to
wire yet): `crates/cs_assets/src/rof.rs` — converting raw records into
VFS `MemberRecord`s is F05-C work, and `tools/cs_inspect/src/rof.rs` —
the `cs-inspect rof` command needs member reads (F05-B/C). Same reasoning
F04-A recorded for not creating `tools/cs_inspect/src/resolve.rs`.

**One observable failure:** with the declared-length validation removed,
`accept_f05_a_name_table_declared_length_mismatch_is_rejected` accepts a
block whose records describe 6 name bytes under a header that declares 7
(and the mirror case: 6 described under 5 declared) instead of rejecting
it. Verified by mutation after implementation (results below).

## Design decisions

- **One block, no traversal.** `read_directory(context, bytes)` parses the
  directory block at the start of the range it is handed: the root is the
  file from offset zero (as the reference extractor reads it), a nested
  directory is the range from its record's `start`. Following those
  `start`s, cycle detection and bounded depth are F05-B, so the acceptance
  fixture supplies the offsets itself and each block still goes through the
  production entrypoint. Consequence, documented in the module doc: error
  offsets are relative to the handed-in range, so a nested block's absolute
  file offset stays with the caller that performed the seek.
- **Validate before charging.** The parse borrows both tables (bounds
  checks only, no allocation), validates the name table against the
  records, and only then books `entry_count * RECORD_BYTES` against the
  parse's allocation budget and builds the `Vec`. Two properties fall out
  of that order: a block the tables disagree about is rejected while the
  ledger is still untouched (so a `ParseContext` can honestly retry those
  bytes — F03-C's contract), and a table that does not fit the budget is
  refused before a `Vec` exists. A hostile `entry_count` with no bytes
  behind it is caught even earlier, by the reader's checked arithmetic —
  `checked_byte_len` then `read_bytes` — which is allocation-free, so it
  reports `UnexpectedEof` at `rof.directory.records` rather than a budget
  error. Both refusals are asserted.
- **`RofError` instead of stretching `ParseErrorKind`.** Structural
  failures keep `RofError::Parse(ParseError)` (container, absolute offset,
  F03 field path, machine kind). The name-table failures are their own
  variants — `name_table_length`, `empty_name`, `unterminated_name`,
  `interior_nul` — because no existing `ParseErrorKind` honestly describes
  "the records describe a different number of name bytes than the header
  declares"; reusing one would make a machine-matched kind lie. Every
  variant carries container, offset and `code()`.
- **Declared lengths are validated, not just NUL-split.** The reference
  extractor splits the whole name table on `0x00` and assigns the results
  to children in record order; it reads each record's `name_length` but
  never uses it, so a table whose bytes and records disagree would be
  silently mis-assigned (or index out of range) there. Spec F05
  non-negotiable #2 asks for exactly this check, so the reader requires
  `sum(name_length) == names_length`, every declared name to end in its
  NUL, no interior NUL and at least one byte per name. The relationship
  `name_length == len(name) + 1` is authored by
  `tools/make_synthetic_fixtures.py` (`len(b'HELLO.TXT') + 1`) and is
  asserted against the shared fixture.
- **Names stay bytes.** `name_bytes` / `RofEntry::name` hand back the name
  without its terminator, borrowed from the input, with no UTF-8
  assumption (spec non-negotiable #2; the reference `.decode()`s and would
  fail on a non-UTF-8 locale).
- **Raw fields are not reinterpreted.** The on-disk words `length` and
  `length_on_disk` are exposed as two independent fields `raw_length` and
  `raw_length_on_disk` (the names the F05 deliverable asks the reader to
  preserve them under) and are never collapsed (non-negotiable #4), and
  unknown flag bits survive verbatim with `has_unknown_bits()` /
  `unknown_bits()` so a consumer can surface `UnsupportedLayout` *before*
  it reads a span (non-negotiable #5). This stage extracts no span at all,
  so it neither rejects nor acts on unknown flags. Extents (`start`,
  `length` against the file length) are likewise recorded, not checked:
  that needs the whole file and belongs to F05-B.
- **The decoded record is exactly the on-disk record.**
  `size_of::<RofRawRecord>() == RECORD_BYTES == 24` is asserted, because
  the allocation model charges `entry_count * 24` and that is only honest
  while the struct has no padding.
- **Fixtures are authored in test code**, never committed as new binaries
  (`fixtures/synthetic/README.md` asks for exactly that). The shared
  `flat-uncompressed.rof` is read through `include_bytes!` and checked
  against values written down from the Python generator's documented
  output, so the fixture's writer and our reader do not share one
  assumption.

## Test inventory (10 tests, prefix `accept_f05_a_`)

| Test | What it pins down |
| --- | --- |
| `accept_f05_a_tree_with_two_directories_duplicate_basenames_and_stable_ids` | AC01 / minimum scenario: root with two directory entries (offsets, ids), both child blocks parsed through production code, `readme.txt` in both directories with ids 11/21, ids identical on a re-read and on an owned copy, exact block lengths 98/77/77 |
| `accept_f05_a_shared_flat_fixture_matches_independent_assertions` | the committed Python-authored fixture: header, names, ids, every record field, `block_len == 76 < 110`, payload bytes verbatim, zero-length entry accepted |
| `accept_f05_a_raw_fields_are_preserved_verbatim` | asymmetric `raw_length`/`raw_length_on_disk` kept apart, unknown flag bit preserved with `unknown_bits()`, observed bits still meaningful, `size_of::<RofRawRecord>() == 24` |
| `accept_f05_a_name_table_declared_length_mismatch_is_rejected` | both directions of the declared-length mismatch, with container/offset/declared/described |
| `accept_f05_a_name_without_terminator_is_rejected` | `unterminated_name`, offset of the name table |
| `accept_f05_a_zero_length_name_is_rejected` | `empty_name` |
| `accept_f05_a_name_with_interior_nul_is_rejected` | `interior_nul` |
| `accept_f05_a_every_truncation_of_a_block_is_rejected` | every prefix shorter than the 98-byte root block fails structurally, the ledger stays at 0 charges, the two reader anchors (`header.entry_count`, `records`) and the exact 72-byte charge on success |
| `accept_f05_a_record_table_beyond_the_allocation_budget_is_refused` | 48-byte budget refuses the 72-byte table (`AllocationBudgetExceeded`, nothing charged), 72-byte budget accepts it exactly |
| `accept_f05_a_hostile_entry_count_is_refused_by_bounds_checks` | `entry_count == u32::MAX` is refused by checked arithmetic, no charge |

## Commands and exit codes

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f05_a_ --include-ignored` | 0 (10 tests, all pass) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f05_a_` | 0 (`10 test(s) selected ... 10 passed`, re-run alone with `--exact`) |

## Mutation probes (implementation removed → tests fail)

Applied one at a time to `crates/cs_formats/src/rof.rs`, `cargo test -p
cs_formats --test rof` (exit 101 each), source restored afterwards:

| Mutation | Failing tests |
| --- | --- |
| `validate_names` returns immediately (validation removed) | name-table mismatch, no terminator, interior NUL, zero-length name (4) |
| `raw_length_on_disk` decoded from the `raw_length` word (fields collapsed) | raw fields preserved (1) |
| `allocation.reserve(...)` removed (no budget charge) | budget refusal, truncation/ledger (2) |
| `id` decoded as `0` (identity not preserved) | raw fields, AC01 tree, shared fixture (3) |

## Recorded unknowns (not guessed here)

1. **Compressed-length semantics** — which of `length` /
   `length_on_disk` (exposed as `raw_length` / `raw_length_on_disk`) is
   stored and which decoded. The reference reads
   `length` for compressed members and ignores `length_on_disk`; no local
   corpus resolves it (spec non-negotiable #4, F05-D blocker).
2. **Directory record length fields** — the reference ignores both for
   directory entries. The fixture authors them as 0 and no code reads
   them; the raw struct preserves whatever a real file holds.
3. **`id` scope and uniqueness** — whether ids are unique per directory,
   per block or per file is unobserved. They are preserved verbatim and
   used as opaque identity; no uniqueness rule is enforced or invented.
4. **Retail name tables** — `sum(name_length) == names_length` is
   required by spec non-negotiable #2 and holds for the one synthetic
   corpus (10 + 10 = 20 bytes). If a real ROF violates it the reader
   fails loudly with `name_table_length` rather than guessing; that would
   be a finding for F05-B/D, not a reason to loosen the check.
5. **Unknown flag bits** — no observed meaning; preserved, rejected
   nowhere in this stage, owed `UnsupportedLayout` by whichever stage
   first reads a span.
6. **Name encoding** — non-UTF-8 names are possible in some locale; bytes
   are preserved and no lossy conversion happens.

These are already covered by the existing F05 pipeline (F05-B traversal
and bounded reads, F05-C mounting/inspection, F05-D retail evidence), so
no new tasks were filed with `create_tasks`.

## Sources used

- `specs/F05-rof-directory-trees-and-compressed-members.md` (whole sheet,
  `### F05-A` in particular) and `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/research/FORMAT-NOTES.md`, "ROF observed structure [S05]", and
  `docs/research/SOURCES.md` S05.
- The reference extractor `extract_rof.py` (S05), re-read for this stage
  to confirm two details: names are assigned to records in table order
  after splitting on `0x00`, and directories are reached by absolute
  `seek(start)`. The blob pinned in `SOURCES.md`
  (`3cd197de1fc6ff00d18cd832160316ec79cd5d8f`) returned HTTP 404 on
  `raw.githubusercontent.com`, so the file was read from the repository's
  `main` head (file history head `214b170bf330041b411634dbb9fb392d54c2db7a`)
  on 2026-09-28. Read-only observation: no code, layout constants or
  content were copied into this repository.
- `tools/make_synthetic_fixtures.py` and
  `fixtures/synthetic/{flat-uncompressed.rof,expected.json,README.md}`
  (protected paths, read only).
- F03-A/B/C (`crates/cs_formats/src/{io,error}.rs`) for the reader,
  budgets and entrypoint this stage builds on.

## Status

Checked by its own tests only: synthetic fixtures cannot certify
original-data behaviour (spec F05, "Evidence and completion"). This stage
does not mount, extract or decompress anything.

## Review (F05-A review pass)

- **Fix applied:** the two length fields were exposed as `length` /
  `length_on_disk`; the F05 deliverable says "Preserve the two length
  fields as `raw_length` and `raw_length_on_disk` until a local corpus
  resolves their meaning". `RofRawRecord` now uses those spec names (the
  on-disk words keep their observed names in the docs and in
  `docs/research/FORMAT-NOTES.md`), with `decode_record`, the test
  builders and the assertions updated to match. No other stage consumes
  this type yet (`F05-B` is still `todo`), so this was the cheapest moment;
  it also puts the "uninterpreted" marker inside every later
  `record.raw_length` read.
- **Reviewer verification:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  `cargo test --workspace --locked`,
  `cargo test --workspace --locked -- accept_f05_a_ --include-ignored`
  (10 tests) and `cargo run -p cs_xtask --locked -- test-select --prefix accept_f05_a_`
  all exit 0 after the fix. Independent mutation probe: making
  `validate_names` return early makes 4 of the 10 tests fail
  (`cargo test -p cs_formats --test rof`, exit 101), source restored.

