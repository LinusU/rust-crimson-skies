# F07-A: INTERP raw records and golden synthetic fixture

Date: 2026-09-28. Task: F07-A "Add INTERP raw records and golden synthetic
fixture" (`specs/F07-interp-loading-script-container.md`, section
`### F07-A`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/interp.rs` (new): `INTERP_ENTRYPOINT`,
  `INTERP_HEADER_BYTES`, `INDEX_ENTRY_BYTES`, `NAME_FIELD_BYTES`,
  `LINE_HEADER_BYTES`, `TERMINATOR_BYTES`, `InterpRawHeader`,
  `InterpRawIndexEntry` (`name_bytes`), `InterpRawLine` (`data_offset`,
  `raw_arguments`), `RawArgument`, `RawArguments`, `InterpRawScript`
  (`end`), `InterpFile` (`header`, `scripts`, `index_end`), `InterpError`
  (`code`, `container`, `offset`, `From<ParseError>`, `Display`, `Error`),
  `read_interp` and the private `read_script`. The signature/version
  constants are reused from `zbd::header` (F06-A), not duplicated.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod interp;`, the
  `pub use interp::{...}` re-exports and one module-doc sentence.
- `crates/cs_formats/tests/interp.rs` (new): an authored container builder
  and seven `accept_f07_a_*` tests.
- This file.

**Not created in this stage:** `crates/cs_content/src/loading.rs` (the
loading-plan adapter is F07-C) and `tools/cs_inspect/src/interp.rs`
(nothing to inspect beyond raw records until F07-B's validation exists).

**One observable failure:** a reader that joins or does not split the
NUL-separated data cannot report `"VALUE"` at offset 177 and `"42"` at
offset 183 as two arguments of the fixture's second line;
`accept_f07_a_shared_fixture_preserves_two_arguments_exactly` fails.

## Design decisions

- **Raw first, validation later.** The reader checks what it must to walk
  the container: the documented signature and version (anything else is an
  `InterpError`, not a guessed variant), bounds of the header, index table,
  each script offset, each line and the zero terminator. It does **not**
  compare `argument_count` with the NUL count, reject data without a final
  NUL, or require a script to end before the next script — those are the
  F07-B validation rules (AC02) and are left to that stage deliberately,
  so a later change adds rejection without changing any raw field.
- **Terminator is one `u32` zero.** The spec says "zero size terminates the
  script"; `tools/make_synthetic_fixtures.py` writes a single 4-byte zero
  and no argument-count word after it. The reader therefore does not read
  an argument count after a zero size. Whether retail files carry anything
  after the zero word is unknown (see below).
- **Lossless arguments.** `InterpRawLine::raw_arguments` splits the stored
  bytes at every NUL and yields each argument with its absolute offset and
  whether a NUL closed it; trailing bytes after the last NUL come out as an
  unterminated argument. Re-concatenation reproduces the stored bytes
  (tested). Nothing is decoded: no encoding is established for names or
  arguments.
- **Origins, not names.** Each script carries its index position, the
  absolute offset of its index entry and its script offset, so equal names
  never collapse (AC03 preview). The timestamp is exposed only as
  `raw_timestamp` (non-negotiable #5).
- **Absolute offsets.** Each script is read with a fresh reader over the
  whole container skipped to its offset, so every error and every line /
  argument offset is a file offset. An out-of-range script offset is an
  `UnexpectedEof` at `interp.scripts[i].offset`.
- **Budgets.** The index is a bounds-checked borrow before anything is
  charged; the script table and each line are reserved against the
  parse's allocation budget before they grow. A hostile `script_count`
  is refused by the checked read with nothing charged (tested).

## Tests (`cargo test --workspace --locked -- accept_f07_a_`)

| Test | Covers |
| --- | --- |
| `shared_fixture_preserves_two_arguments_exactly` | AC01 on `fixtures/synthetic/synthetic.interp`: header, name, offsets, both lines, `VALUE`/`42` bytes and offsets, terminator at 186 |
| `argument_boundaries_survive_spaces_and_empty_arguments` | `"a b","c"` vs `"a","b c"`, empty and unterminated arguments, lossless reassembly |
| `equal_names_keep_distinct_origins` | two `twin` scripts keep index, entry offset and script offset |
| `name_field_is_kept_verbatim` | padding after the name NUL survives in `name_field` |
| `undocumented_header_is_rejected` | `bad-version.interp` (999 at offset 4) and a wrong signature |
| `every_truncation_is_rejected` | every strict prefix of the fixture; missing terminator reported at `interp.scripts[0].lines[2].size`, offset 186 |
| `out_of_range_offset_and_hostile_count_are_refused` | script offset past EOF; `script_count = u32::MAX` with zero allocation charged |

Mutation probes (each reverted): no NUL split → 2 tests fail; version
check removed → 1 fails; stopping a script after one line → 3 fail;
timestamp not preserved → 1 fails.

## Unknowns (not guessed)

- Whether retail `interp.zbd` matches this layout at all is unmeasured:
  the layout is from the pinned mech3ax source [S07] and the authored
  fixture. F07-D (retail) measures it.
- Whether retail scripts carry bytes after the zero terminator, overlap,
  share offsets or leave unreferenced regions; F07-B reports those as
  findings.
- The encoding of names and arguments, and whether name padding is always
  zero.
- The meaning of the timestamp word (units, epoch).
- Which commands load resources (F07-C/F07-D); the fixture's `SYNTHETIC`
  and `VALUE` commands are invented and never registered as opcodes.

These are already the scope of F07-B and F07-D, so no new tasks were
filed.

## Sources

`specs/F07-interp-loading-script-container.md`,
`docs/research/FORMAT-NOTES.md` ("INTERP observed subset" [S07]),
`docs/research/SOURCES.md` (S07), `fixtures/synthetic/README.md`,
`fixtures/synthetic/expected.json`, `tools/make_synthetic_fixtures.py`,
`docs/contracts/SCRIPT-MISSION.md`.
