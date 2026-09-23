# F03-A: the checked reader, the structured error, and what is still unknown

Date: 2026-09-23. Task: F03-A "Implement checked reader and structured errors"
(`specs/F03-bounded-binary-parsing-primitives.md`). Capabilities used: ordinary
build/test only; `CS_GAME_DIR` was not read and no original data appears in
this change.

## What was built

| file | contents |
|---|---|
| `crates/cs_formats/src/io.rs` | `Reader<'a>`: bounds-checked little-endian reads (`u8..u64`, `i8..i64`, `f32`, `f64`), `read_bytes`, `skip`, `sub_reader`, `read_str`, `read_bounded_cstr`, `checked_byte_len`, `checked_extent` |
| `crates/cs_formats/src/error.rs` | `ParseError { container, offset, field, kind, expected, observed }` and `ParseErrorKind { UnexpectedEof, LengthOverflow, InvalidEncoding, MissingTerminator }` |
| `crates/cs_formats/tests/` | `common/mod.rs` synthetic fixture plus three `accept_f03_a_*` integration-test binaries (8 tests) |

One synthetic fixture (`tests/common/mod.rs`) is the typed input/output pair
for this stage: a 23-byte little-endian record and `parse_record`, the entry
point every truncation test drives. It is newly authored data.

## Design decisions (engineered, not observed)

These are project-designed primitives; no claim about any original file layout
or string encoding is made or implied:

1. **No `unsafe`, no `transmute`.** Every integer/float is decoded with
   `from_le_bytes` over a copied byte array, so host endianness and slice
   alignment cannot affect results (`accept_f03_a_reads_deliberately_
   unaligned_slices` parses a record at an odd address).
2. **Bounded c-strings consume their whole bound.** `read_bounded_cstr` takes
   exactly `max_len` bytes, then requires a `0x00` inside them and valid UTF-8
   in the prefix. A terminator is therefore never optional, and a truncated
   field cannot be silently accepted as a shorter string. Whether retail
   strings are NUL-terminated, length-prefixed or fixed-padded per format is
   still **unknown** and belongs to the per-format tasks (F05+); only the
   primitive exists here.
3. **Errors carry metadata only.** `expected`/`observed` hold counts, indices
   and offsets; payload bytes are never copied into an error, which
   `accept_f03_a_string_bounds_report_errors_without_payload` asserts by
   feeding a label containing `SECRET!!` and checking the rendered message.
4. **Sub-readers rebase offsets, keep provenance.** A failure inside a nested
   range reports its absolute offset in the container (asserted at offset 5
   inside a member range).
5. **Checked arithmetic precedes slicing.** `checked_byte_len` (`count *
   element_size`, e.g. `u32::MAX * 8`) and `checked_extent` (`offset + length`)
   return `LengthOverflow` before any slice exists; `read_bytes` only ever
   hands out a borrow of the input, so no length can cause an allocation.

## Sensitivity evidence

With the bounds check in `Reader::take` disabled (`if false && available <
len`), `cargo test -p cs_formats -- accept_f03_a_` exits **101** with
`accept_f03_a_u32_max_count_is_bounded_without_allocation ... FAILED` (the
harness stops at the first failing binary; the truncation binaries depend on
the same check). The mutation was reverted; tests are green again. This
matches the sheet's rule that tests must fail when the implementation is
removed.

## Commands run (all exit 0 unless noted)

| command | exit |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f03_a_ --include-ignored` | 0 (8 tests) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f03_a_` | 0 (8 selected, 8 re-ran `--exact`) |
| mutation probe described above | 101 (expected failure) |

## Open point for a later stage (recorded, not guessed)

`docs/contracts/IDENTITY-CONTENT.md` gives `SourceSpan` an `install_sha256`.
`cs_formats` is dependency-free apart from `cs_types` and performs no IO, so
`ParseError` carries the container label, offset, field and conditions but
**not** an installation hash. Converting a `ParseError` into a `SourceSpan`
(needing the install hash and optional member hash) is left to the consumer in
a later integration stage; no hash value is invented here.
