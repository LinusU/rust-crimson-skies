# F05-B: directory traversal and bounded member reads

Date: 2026-09-28. Task: F05-B "Implement directory traversal and bounded
member reads" (`specs/F05-rof-directory-trees-and-compressed-members.md`,
section `### F05-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/rof.rs` (extended): `TREE_ENTRYPOINT`,
  `DECODE_CHUNK_BYTES`, `RofLimits` (`DEFAULT_MAX_DECODED_BYTES`, `new`,
  `Default`), `RofTreeDirectory`, `RofMember`, `RofTree` (`root`,
  `directories`, `members`), `RofMemberRead` (`data`, `stored_len`,
  `trailing_len`, `decoded_len`), the private `DirectoryPlan`, `Plan`
  (`booking_bytes`, `into_tree`, `check_overlaps`), `Walker` (`new`,
  `visit`), `read_tree`, `read_member`, `decode_zlib`; the five new
  `RofError` variants `Cycle`, `ExtentOutOfBounds`, `ExpansionBomb`,
  `UnsupportedLayout`, `DecodeFailure` (each with `code()`, `container()`,
  `offset()` and a `Display` line) plus the private `RofError::shifted`;
  and a `borrow_block` / `BlockView` / `decode_table` refactor that lets
  `read_directory` and `read_tree` validate a block through the same code
  without changing `read_directory`'s behaviour.
- `crates/cs_formats/src/lib.rs` (wiring only): the `pub use rof::{...}`
  re-exports gained `TREE_ENTRYPOINT`, `RofLimits`, `RofMember`,
  `RofMemberRead`, `RofTree`, `RofTreeDirectory`, `read_member`,
  `read_tree`, plus one module-doc sentence naming F05-B.
- `crates/cs_formats/Cargo.toml` and `Cargo.lock` (wiring only): one new
  dependency, `miniz_oxide = "0.9"` (rationale in the design decisions).
- `crates/cs_formats/tests/rof.rs` (extended): the three compressed
  fixtures (`COMPRESSED_PAYLOAD`, `COMPRESSED_STREAM`, `BOMB_STREAM`), the
  builders (`single_member_file`, `root_directory_pointing_at`,
  `outside_file_member`, `unknown_flag_member`, `overlapping_members`,
  `directory_chain`, `member_of`) and eleven `accept_f05_b_*` tests.
- `docs/findings/2026-09-28-f05-b-directory-traversal-and-bounded-member-reads.md`
  (this file).

**Not created in this stage** (owner paths of F05-C, nothing to wire yet):
`crates/cs_assets/src/rof.rs` (records to VFS `MemberRecord`s) and
`tools/cs_inspect/src/rof.rs` (the `cs-inspect rof` command needs the
mount F05-C builds) — exactly what F05-A recorded.

**One observable failure:** with the read profile switched from the
record's `raw_length` extent to `raw_length_on_disk`,
`accept_f05_b_compressed_member_stored_and_decoded_lengths_differ` fails:
the decoder is handed 32 of the 62 stored bytes, reports
`decode_failure` instead of the 204-byte payload, and AC02's "stored and
decoded lengths differ, the profile explains both" collapses. Verified by
mutation after implementation (table below).

## Design decisions

- **The read profile, in one sentence.** *Stored* = the record's
  `raw_length` bytes at `start`, established against the container length
  before a single byte reaches the decoder — the field the reference
  extractor reads for compressed members ([S05]); *decoded* = what the
  bounded zlib decoder produces, which is where the decoded length comes
  from because no record field states it. `RofMemberRead` reports both
  (`stored_len`, `decoded_len`) plus `trailing_len`, the bytes the stream
  did not consume (non-negotiable #4 asks for exact boundaries *and*
  trailing data). `raw_length_on_disk` is validated as an extent like
  every other declared length and then never read: which of the two words
  is stored and which decoded is the F05-D blocker, and nothing here
  decides it from an English field name. The AC02 test proves the profile
  is load-bearing: it flips the two words on a hand-built member and gets
  a `decode_failure`, and it re-reads the same stream through three
  different `raw_length_on_disk` values and gets byte-identical output.
- **Walk, then book once, then build — all inside one `parse` attempt.**
  Validation collects borrowed views and paths while nothing is charged,
  so every failure path (structural, domain or budget) leaves the
  allocation ledger exactly as it found it (F03-C's retry contract) and a
  budget refusal never keeps a partial tree. The single reservation
  covers everything the returned tree keeps: every record table
  (`total_records * RECORD_BYTES`, the bytes `read_directory` books for
  one block), both node arrays (`size_of::<RofTreeDirectory>()`,
  `size_of::<RofMember>()`) and the path slice headers
  (`size_of::<&[u8]>()` per segment) — one `reserve` call, because the
  budget has no release operation and a partial booking could never be
  undone. The reservation must live inside the attempt because
  `ParseContext::allocation()` only hands out a shared reference; the
  entrypoint scopes its failure as `rof.tree.records`.
- **Cycle detection by ancestor chain, shared blocks by a byte budget.**
  The offsets open on the path from the root are checked before a child
  block is entered, so a loop reports `RofError::Cycle` with the depth
  that was open (1 for a self-loop, 2 for `root -> SUB -> root`) instead
  of running to the recursion limit. A block reached from a *second*
  parent is not a cycle but is still refused: the walk accumulates the
  `block_len` of every visited block and refuses once that sum passes the
  container length, because disjoint blocks can never reach it. That one
  check bounds the whole walk to work linear in the container however the
  records point (blocks reinterpret each other's bytes), and the same
  sharing also shows up in the overlap check below — two independent
  detectors, no global visited set to allocate.
- **Depth is F03's recursion budget, not a new constant.** Every level
  enters `RecursionBudget::enter("directory", offset)`, so nesting past
  the parse's `max_depth` fails structurally as
  `ParseErrorKind::RecursionDepthExceeded` scoped `rof.tree.directory`
  with the absolute offset of the block that did not fit, and the guards
  unwind on both the success and the error path.
- **Extents before bytes, both words, for every record.** Each record's
  `raw_length` *and* `raw_length_on_disk` must end inside the container
  — directories included, because a directory record that points past the
  end is exactly the "outside-file pointer" AC03 refuses, and refusing it
  costs nothing even though the meaning of those two words on a directory
  is unknown. A member handed to `read_member` is checked again, so a
  hand-built member cannot read outside the container either.
- **Flags decide before spans, and only three combinations exist.** Bits
  outside `KNOWN_FLAG_MASK` and the unobserved directory+compressed
  combination surface `RofError::UnsupportedLayout` while the entry is
  still just a record (spec non-negotiable #5: absent documented
  legitimate sharing, refuse rather than extract). `read_member` refuses
  the same words again — including a directory record, which has no
  member bytes to read — so the rule holds for callers that never went
  through `read_tree`.
- **Overlap is checked once, after the walk, on non-empty spans only.**
  Every block span and every member's `raw_length` extent is sorted and
  compared pairwise (linearithmic); two spans that share a byte are an
  `UnsupportedLayout`. The `raw_length_on_disk` extent is deliberately
  *not* compared — it is never read, and comparing it would reject a
  container for a word whose meaning is unresolved. Empty members are
  excluded because they extract nothing (which is what lets the committed
  fixture's zero-length entry read successfully).
- **The bounded zlib decoder is `miniz_oxide`.** Non-negotiable #3 wants
  "a bounded zlib decoder only after the entry extent is established":
  `read_member` establishes the extent first, then `decode_zlib` streams
  it through `miniz_oxide`'s `InflateState` (`DataFormat::Zlib`, so the
  header and adler32 trailer are checked) writing into a fixed 8 KiB chunk
  buffer, comparing the running total against
  `RofLimits::max_decoded_bytes` *before* each append — an expansion bomb
  stops at the ceiling and never materialises past it. Only bytes the
  decoder reports as consumed advance the input cursor, which is how
  `trailing_len` is measured exactly; a `Buf` error (stream ends before
  its data does) and a `Data` error (corrupt data or checksum mismatch)
  both become `RofError::DecodeFailure` with no partial output, and a
  round that moves no bytes is refused instead of spinning.
  **Why a dependency:** `cs_formats` is a parser crate with no codec of
  its own and the architecture table's dependency column governs *project*
  crates (`cs_types`), which is unchanged; `miniz_oxide` is pure Rust (no
  C toolchain, no system zlib), was already in `Cargo.lock` through the
  Bevy stack, and — the deciding point — its `inflate` reports exact
  `bytes_consumed` / `bytes_written`, which the trailing-data requirement
  needs. Hand-writing DEFLATE here would have been a larger, less verified
  implementation of exactly the kind `FORMAT-NOTES.md` warns about.
- **Fixtures are produced by an implementation we do not link.** The
  compressed streams (62-byte stream for a 204-byte payload; 149-byte
  stream for 128 KiB of zeros) were compressed by Python 3.14's zlib 1.2.12
  and are pasted as constants with their generator command next to them, so
  the writer and the decoder share no code (`FORMAT-NOTES.md`: a fixture
  whose writer and reader share one assumption proves nothing).
- **Member buffers are not charged to the parse budget.** The budget has
  no release operation, so charging every decoded member would eventually
  make an honest multi-member container refuse to read. `read_member`
  takes `RofLimits` explicitly instead: the per-read decoded ceiling is
  the bound that can be enforced exactly, and the tree booking above is
  what `allocation()` accounts for.
- **Absolute offsets out of the walk.** `borrow_block` reports offsets
  relative to the range it was handed (F05-A's contract); `read_tree`
  shifts every failure raised inside a nested block onto the absolute
  container offset and adds the `directory` scope to structural ones, so
  a nested failure reads `rof.tree.directory.<field>` at the real offset
  while `read_directory` keeps its block-relative behaviour untouched.

## Test inventory (11 tests, prefix `accept_f05_b_`)

| Test | What it pins down |
| --- | --- |
| `accept_f05_b_traversal_finds_two_directories_duplicate_basenames_and_stable_ids` | AC01 through the production walk: root + two directory blocks with their paths and offsets, all five members depth-first with ids 11/12/21/22/3, duplicate `readme.txt` under two directories kept apart by id, both declared extents inside the container, identical listing on a re-read |
| `accept_f05_b_uncompressed_member_reads_are_byte_identical` | every authored payload read back byte for byte with `stored_len == decoded_len` and no trailing data; the committed Python-authored fixture through the same path (34-byte payload verbatim, the zero-length member reads zero bytes instead of failing) |
| `accept_f05_b_compressed_member_stored_and_decoded_lengths_differ` | **AC02 / minimum scenario**: 62 stored vs 204 decoded with both reported and neither taken from the wrong word; `raw_length` (62) ≠ `raw_length_on_disk` (32, in bounds); flipping the words makes the read fail with `decode_failure` at the member's offset; three different `raw_length_on_disk` values read byte-identically |
| `accept_f05_b_compressed_extent_records_trailing_data_and_refuses_bad_streams` | an extent 4 bytes longer than the stream reports `trailing_len == 4` with the full payload; a 3-byte-short stream and a corrupted byte both fail `decode_failure` with no partial output; an entry with an unexplained flag bit is refused before its span is read |
| `accept_f05_b_expansion_bomb_fails_at_the_configured_ceiling` | 149 stored bytes decode to 128 KiB under the default ceiling; at 4 KiB it fails `expansion_bomb` with `limit`, `observed` past the limit but never more than one chunk past it; a ceiling of 0 refuses; member reads book nothing |
| `accept_f05_b_directory_cycles_and_shared_blocks_are_refused` | AC03 cycle: a self-loop (`depth` 1) and `root -> SUB -> root` (`depth` 2) both fail `cycle` at offset 0; one block under two parents fails `unsupported_layout` at the shared block; ledger untouched in all three |
| `accept_f05_b_bounded_depth_refuses_to_descend_forever` | a 4-block chain reads under the default budget; a context with `max_depth = 2` refuses the third block with `RecursionDepthExceeded`, field `rof.tree.directory`, at offset 68 |
| `accept_f05_b_outside_file_pointers_fail_before_any_read` | AC03 outside-file: a member `length` past the end, `raw_length_on_disk` past the end while `length` fits, a directory pointer at offset 5000 (a block needs its 8 header bytes), and a hand-built member handed straight to `read_member` — all `extent_out_of_bounds` with the offending length and `file_len` |
| `accept_f05_b_invalid_name_table_fails_a_nested_block` | AC03 invalid name table *inside a nested block*: `name_table_length` at the absolute offset (root block + header + record), declared 4 vs described 3, ledger untouched |
| `accept_f05_b_unexplained_flags_and_overlaps_surface_unsupported_layout` | non-negotiable #5: flag bit `0x8` (the refusal names `0x00000008`), the directory+compressed combination, and two members sharing 30 bytes refused at offset 220 with "overlap" in the message |
| `accept_f05_b_bookings_and_refusals_leave_the_ledger_exact` | the exact booking recomputed from the returned tree; one byte less refuses as `AllocationBudgetExceeded` at `rof.tree.records` with nothing charged; exactly enough charges once; four different refusals leave a fresh ledger at 0 and a charged one unchanged; member reads (success and bomb) book nothing |

## Commands and exit codes

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f05_b_ --include-ignored` | 0 (11 tests) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f05_b_` | 0 (`11 test(s) selected ... 11 passed`, re-run alone with `--exact`) |
| `cargo test -p cs_formats` (F05-A + F06-A + doctests) | 0 (10 + 11 + 4 tests) |

## Mutation probes (implementation removed → tests fail)

Applied one at a time to `crates/cs_formats/src/rof.rs`, each run as
`cargo test -p cs_formats --test rof`, source restored afterwards (the
suite was green again after every restore):

| Mutation | Exit | Failing tests |
| --- | --- | --- |
| cycle check disabled (`ancestors.contains` never true) | 101 | `...directory_cycles_and_shared_blocks_are_refused`, `...bookings_and_refusals_leave_the_ledger_exact` |
| both member extent bounds disabled | 101 | `...outside_file_pointers_fail_before_any_read`, `...bookings_and_refusals_leave_the_ledger_exact` |
| only the `length_end` bound disabled | 0 | *none* — the `length_on_disk_end` check still refuses that fixture, which is why both words are checked |
| read profile flipped to `raw_length_on_disk` as the stored extent | 101 | `...compressed_member_stored_and_decoded_lengths_differ` (the AC02 headline) |
| expansion-bomb ceiling disabled | 101 | `...expansion_bomb_fails_at_the_configured_ceiling` |
| overlap check disabled | 101 | `...unexplained_flags_and_overlaps_surface_unsupported_layout`, `...directory_cycles_and_shared_blocks_are_refused`, `...bookings_and_refusals_leave_the_ledger_exact` |
| unknown-flag refusal disabled | 101 | `...unexplained_flags_and_overlaps_surface_unsupported_layout`, `...bookings_and_refusals_leave_the_ledger_exact` |
| tree booking removed (`allocation.reserve` dropped) | 101 | `...bookings_and_refusals_leave_the_ledger_exact`, `...expansion_bomb_fails_at_the_configured_ceiling` |
| bounded depth disabled (`enter(...).ok()` swallows the limit) | 101 | `...bounded_depth_refuses_to_descend_forever` |

## Recorded unknowns (not guessed here)

1. **Compressed-length semantics** — which of `raw_length` /
   `raw_length_on_disk` is stored and which decoded stays unresolved
   (F05-D blocker, inherited from F05-A). This stage selects the
   *observed* profile (the reference reads `length` for compressed
   members and ignores `length_on_disk`, [S05]) and reports both lengths
   of every read rather than declaring either field solved.
2. **Directory record length fields** — validated to lie inside the
   container, meaning still unknown (F05-A's unknown #2). If a retail
   directory carries a length larger than its container the walk fails
   loudly with `extent_out_of_bounds`; that would be a finding for F05-D,
   not a reason to drop the check.
3. **`raw_length_on_disk` of a member** — validated, then never used as
   an extent and excluded from overlap comparisons. A retail corpus that
   shows it *is* the stored extent changes the profile in exactly one
   place (`read_member`), which is why the profile is stated in one
   sentence in its docs.
4. **Retail flag words and legitimate sharing** — only `0`, `1` and `2`
   are acted on, and no overlap is documented as legitimate, so a retail
   file using something else fails `unsupported_layout` with the record's
   offset and the numeric detail. Nothing is guessed: the failure names
   the numbers only, never a member name from the container.
5. **The visit-bytes cap** (`visited block_len` summed, refused past the
   container length) is a *designed* bound derived from the container
   size, not an observed original value; it is what makes the walk's work
   linear without a global visited set.
6. **Name encoding** — unchanged from F05-A: bytes, no lossy conversion.

No new tasks were filed: these are already carried by the F05 pipeline
(F05-C mounting, F05-D retail evidence).

## Sources used

- `specs/F05-rof-directory-trees-and-compressed-members.md` (whole sheet,
  `### F05-B` in particular) and `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/research/FORMAT-NOTES.md`, "ROF observed structure [S05]"
  (root at offset zero, directory recursion by absolute seek, compressed
  members handed to zlib, the reference reading `length` and ignoring
  `length_on_disk`, and the production-reader list of bounds/recursion/
  duplicate handling this stage implements) and `docs/research/SOURCES.md`
  S05.
- `docs/findings/2026-09-28-f05-a-raw-rof-structs-and-boundary-fixtures.md`
  for the recorded unknowns this stage inherits and the F05-A conventions
  (errors with code/container/offset, booking before building) it keeps.
- `crates/cs_formats/src/{io,error}.rs` (F03-A/B/C) for `ParseContext::parse`,
  `AllocationBudget`, `RecursionBudget` and `ParseError::in_scope`.
- `miniz_oxide` 0.9.1 source read locally in the cargo registry
  (`src/inflate/stream.rs`, `src/inflate/mod.rs`) to confirm the exact
  `inflate` contract this code relies on: `MZFlush::None` semantics,
  `bytes_consumed` accounting, `Err(MZError::Buf)` on a stream that ends
  early and `Err(MZError::Data)` on an adler32 mismatch. No code was
  copied; only the API contract was read.
- Python 3.14.6 / zlib 1.2.12 for the compressed fixtures, invoked as
  `python3 -c "import zlib; ..."`, command recorded next to each constant
  in the test file.
- `tools/make_synthetic_fixtures.py` and
  `fixtures/synthetic/{flat-uncompressed.rof,expected.json}` (protected
  paths, read only), exercised end-to-end through `read_tree` +
  `read_member`.

## Status

Checked by its own tests only: synthetic fixtures cannot certify
original-data behaviour (spec F05, "Evidence and completion"). This stage
reads no original data; the retail compressed-length evidence, the VFS
mount and the `cs-inspect rof` command belong to F05-C and F05-D.
