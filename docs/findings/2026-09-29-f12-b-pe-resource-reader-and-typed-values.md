# F12-B: the PE resource reader, the resource-id header reader and typed configuration values

Date: 2026-09-29. Task: F12-B "Implement confirmed text and PE resource
readers" (`specs/F12-text-configuration-strings-and-pe-resources.md`, section
`### F12-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Required capability: ordinary build/test. This machine also has `retail`, and
it was used **read-only** for the survey below (spec rule 1: "Extract grammar
from real samples before implementing a parser"). Nothing from the
installation is committed except paths, lengths, hashes, ids, counts, code
pages and sizes.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/pe_resources.rs` (new): `PeLayout`, `PeSection`,
  `DataDirectory`, `RvaSpan`, `ResourceKey`, `ResourceData`, `ResourceLeaf`,
  `StringUnit`, `StringBlock`, `PeResources`, `PeError`, `read_pe_layout`,
  `read_pe_resources`, `string_id`, and the format constants.
- `crates/cs_formats/src/text/resource_header.rs` (new): `ResourceHeader`,
  `ResourceHeaderLine`, `ResourceHeaderKind`, `Define`, `ResourceIdValue`,
  `HeaderLookup`, `read_resource_header`, `MAX_RESOURCE_ID`.
- `crates/cs_formats/src/text/dialect.rs`: `DialectReader` gains
  `ResourceHeader` and `PeResources` (with `entrypoint()`), and the two rows
  that F12-A deferred to `F12-B` now name their readers.
- `crates/cs_formats/src/text/mod.rs`, `crates/cs_formats/src/lib.rs`: wiring
  only — module declarations, re-exports, doc paragraphs.
- `crates/cs_formats/src/pe_resources_tests.rs` (new) and
  `crates/cs_formats/src/text/tests_f12_b.rs` (new): the `accept_f12_b_*`
  tests, two of them retail (`#[ignore = "requires CS_GAME_DIR"]`).
- `crates/cs_content/src/config.rs`: `ValueWidth`, `FieldSpec`, `TuneError`,
  `Tuning`, `TuningSchema`, and `ConfigEntry::line`.
- Wiring only: `crates/cs_content/src/lib.rs` (a doc paragraph).

**Not created in this stage:** `tools/cs_inspect/src/config.rs`. An inspect
command for a configuration member needs a mounted ROF member to read, and
`cs-inspect rof` already provides that; the command is developer-facing
diagnostics over the *document layer*, which is F12-C's wiring of the reader
into its producer and consumer, not a new format. F12-C's minimum scenario is
AC03, which is exactly this path.

**One observable failure:** a reader that does not bounds-check a data entry's
RVA against its section hands back bytes that are not there, or follows a
directory that points at itself and never terminates. Both are pinned by
`accept_f12_b_pe_resource_offsets_are_cycle_and_bounds_checked`, which was
checked by mutation (below).

## Survey: the three PE images

Read-only structural walk of the resource directory of each surveyed image.
Only counts, ids, code pages and sizes are recorded; no string text is.

| File | Bytes | PE | Sections | `.rsrc` rva / size | Types (id, leaves) | Directories / leaves |
| --- | --- | --- | --- | --- | --- | --- |
| `strings.dll` | 131 072 | PE32, `machine 0x014c` | 5 (`.text`, `.rdata`, `.data`, `.rsrc`, `.reloc`) | `0x12000` / 46 712 | 6 `RT_STRING` (112), 16 `RT_VERSION` (1), 255 unassigned (1) | 118 / 114 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 32 768 | PE32, `machine 0x014c` | 5 | `0x6000` / 2 624 | 6 `RT_STRING` (3) | 5 / 3 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 282 624 | PE32, `machine 0x014c` | 2 (`.rsrc`, `.reloc`) | `0x1000` / 270 344 | 6 `RT_STRING` (101) | 103 / 101 |

Language ids and code pages, over every leaf of every image:

| File | Language id | Code page | Notes |
| --- | --- | --- | --- |
| `strings.dll` | `1033` (`0x0409`, en-US) on all 114 leaves | `1252` on all 114 | the only image whose data entries name a code page |
| `language.dll` | `1033` on all 3 | `0` | the resource compiler's "no code page" value |
| `langui.dll` | `1033` on all 101 | `0` | id `1` appears in the *directory entry counts*, not as a language: the corrected walk shows every third-level id is `1033` |

Corrections to the F12-A inventory, both from this survey:

- F12-A recorded `langui.dll`'s language as unknown. It is `1033` like the
  other two; the earlier reading of the directory headers was wrong and this
  walk supersedes it.
- `strings.dll` carries **two resource types besides `RT_STRING`**: type `16`
  (`RT_VERSION`) and an unassigned type `255`, one leaf each. A reader that
  only walked `RT_STRING` would have reported 112 leaves where the image holds
  114, and would have discarded two resources without counting them — exactly
  the "unknown keys are retained and counted" rule of spec F12, applied to the
  resource tree.

Every observed `RT_STRING` block is a whole number of counted UTF-16LE units of
at most sixteen per block, with no trailing bytes, and every unit of the
English installation decodes to text. No directory entry at any level in any
of the three images uses a *name* rather than an id.

## Survey: the two `.H` members

Read through `cs-inspect rof` into `private/` (git-ignored), then analysed
structurally. Lengths and hashes are F12-A's; the shape counts are new.

| Member | Bytes | Lines | `#define` | `//` comments | blank | other directives |
| --- | --- | --- | --- | --- | --- | --- |
| `ASSETS/SCRIPTS/RESOURCE.H` | 29 579 | 646 | 635 | 6 | 2 | 4 |
| `ASSETS/SCRIPTS/RESRC1.H` | 8 922 | 196 | 185 | 6 | 2 | 4 |

Observed, in both members: CRLF only, terminated; 7-bit ASCII only; every
`#define` value is a **plain decimal** (no `0x`, no expression, no sign); every
value is at most five digits and at most `40 201`, well inside the 16-bit
resource-id space; every name is unique exactly and case-insensitively; values
repeat (the APS trailer ids are shared between the two members, and F12-A
counted 612 distinct values among `RESOURCE.H`'s 635 lines). The four other
directive lines are `#ifdef NAME`, `#ifndef NAME`, `#endif`, `#endif` — the
include guard, which is why `RESOURCE.H`'s `__MIT_H__` is itself a `#define`
with no value.

**Not observed:** a fractional value, a signed value, a hexadecimal value, a
`#define` without a name, a duplicate name, a line without a terminator, any
byte above `0x7F`.

## Design decisions

### The PE resource reader

- **Two passes, the first allocation-free.** Pass one reads the headers, walks
  the whole tree and checks every string block; it allocates nothing at all —
  the section headers are read on demand and the walk's two stacks
  (`WalkPath`) live on its frame. Pass two books and builds the records. A
  domain refusal therefore leaves the ledger exactly as it was, with no
  rollback needed, and every offset the build pass uses has already been
  checked. This is the same discipline the ROF reader uses
  (`RofError::in_block`, "structural failures carry their own code and
  offset").
- **Bounds are checked against the container, not the file.** Every directory
  table, 8-byte entry, UTF-16 name and 16-byte data entry is checked against
  the resource directory's *declared size*; a table claiming more entries than
  the section holds is refused. A leaf's data is located through the section
  table and checked against that section's *raw* bytes, so an RVA inside a
  section's uninitialised tail (`VirtualSize > SizeOfRawData`) yields nothing
  rather than a read past the file.
- **Cycles are detected, not survived.** `WalkPath` holds the directory offsets
  on the current path; a directory already on it is `PeError::DirectoryCycle`
  before a byte of it is read. The nesting is bounded twice, by the parse's
  `RecursionBudget` and by the module's own `MAX_RESOURCE_DEPTH` (32, a
  `Designed` value), so the walk's memory does not grow with a hostile tree
  however wide the parse's budgets are.
- **Depth is not fixed.** The three surveyed images nest three levels, but the
  format does not require it, so a leaf keeps its whole `path` as written and
  only a *three-level* `RT_STRING` leaf with a numberable block id is read as
  a string block. The fixture carries a four-level `RT_STRING` leaf to pin
  this.
- **Language and code page are retained verbatim, never interpreted.** `0` is
  kept as the code page the resource compiler wrote; it is not replaced by a
  default. A block is identified by `(block_id, language)`, because block 1
  exists once per language in the fixture.
- **Unpaired surrogates are recorded, not replaced.** `StringUnit::text` is
  `None` and `code_units` keeps the exact units.
- **`Reader` is not used for the tree walk.** `Reader` is a forward-only cursor
  and `io.rs` is not an owner path of this stage, so the module's `Image`
  performs the same checks directly (checked extent before every slice, byte-wise
  little-endian decoding, no `unsafe`) and builds the same `ParseError`s. The
  follow-up that would give `Reader` an absolute-offset window is filed.
- **`MAX_RESOURCE_DEPTH` is `Designed`**, like `RecursionBudget::DEFAULT_MAX_DEPTH`.

### The resource-id header reader

- **Every line is a node, and every byte survives** (`reassemble` is exact).
- **The value's `raw` keeps its separator blank bytes**; `text()` trims them.
  This is the same distinction F12-A made between the key's padding and the
  key.
- **Only a plain decimal of at most five digits is a resource id.**
  `0x10`, `65536`, `-1`, `1+2`, `4294967296` and `007` all keep their bytes
  and name no id: the first two are outside the observed shape and the id
  space, the rest are spellings the survey never found. `007` *is* a plain
  decimal and does name id 7, because the survey cannot settle whether
  leading zeros occur and a decimal is a decimal.
- **A duplicate name is `Ambiguous`**, never a silent overwrite, matching
  `ConfigDocument::lookup`.
- **The id space is a shared one.** A `.H` `#define` names an id in
  `0..=65535`; a `RT_STRING` unit's id is `(block - 1) * 16 + index`, which for
  the full 16-bit block range reaches `1_048_559`. The two readers agree on
  what a *decimal* means and disagree on the *range*, and
  `accept_f12_b_dialect_inventory_points_at_the_new_readers` pins that rather
  than hiding it.

### The typed configuration values

- **Width and signedness belong to the schema, not the value.** A value is
  only ever converted against a `FieldSpec`, so a consumer cannot read a
  negative count or an over-wide tuning number into a Rust type that would
  accept it (spec non-negotiable #2).
- **A `Tuning` exists only for a value that passed every check.** Its presence
  *is* the proof; a failure is a `TuneError` and the bytes stay in the
  document. AC02's three cases each have their own code: `negative`,
  `overflow`, `not_finite`.
- **A fractional value is an overflow, not a truncation.** `1.5` against an
  integer spec is refused, so no tuning constant is ever a silently rounded
  value.
- **The approved range is the numeric contract's condition** for a tuning
  float value: `min`/`max` are that range, `None` means "finiteness only", and
  a consumer that needs a bound declares one.
- **`nan`, `inf` and an exponent are not spellings**, so a non-finite value
  cannot be written; the finiteness rule in `tune` is the backstop for digits
  that overflow `f64` to an infinity.
- **A fractional spelling is `Inferred`, not observed.** The survey found no
  fractional value in either member, so `5.5` reads because of a *designed*
  rule. This is recorded below as a follow-up, not presented as a measurement.
- **A tuning float's unit is a label, not a conversion.** `FieldSpec::unit` is
  carried into `Tuning` for a consumer's own conversion; this module performs
  no unit conversion at all, because the contract requires original units to be
  established before any is applied.

## Test inventory (`accept_f12_b_*`)

| Test | Covers |
| --- | --- |
| `pe_resources_tests::…pe_resource_tree_keeps_ids_languages_and_code_pages` | ids, names, language ids, code pages, two blocks under one id, a four-level leaf, a non-`RT_STRING` payload left alone, every RVA resolved inside a section |
| `…pe_resource_offsets_are_cycle_and_bounds_checked` | self-referential and two-level cycles, a subdirectory past the section, a table past the declared size, an over-long name, an RVA in a section's uninitialised tail, an over-long data extent, an unmapped RVA, an over-large and a truncated resource directory, an unmapped directory RVA; a refusal books nothing |
| `…string_blocks_are_bounds_checked` | a unit past the block, a truncated length word, trailing bytes counted, block id `0`, an over-wide block id, an unpaired surrogate |
| `…pe_layout_and_directory_presence_are_checked` | an image with no resource directory, no `MZ`, no `PE\0\0`, an unknown optional-header magic, a truncated image, `map_rva` for a section, the headers and nothing |
| `…pe_resources_are_bounded_by_the_allocation_budget` | the exact budget reads, one byte less is refused with `allocation_budget_exceeded` and rolls back, budget `0`, recursion `0`, recursion `2` against a `9`-level tree |
| `…retail_pe_resource_structure_matches_the_survey` (ignored, retail) | the three images' lengths, PE shape, `.rsrc`, directory and leaf counts, the three types of `strings.dll`, language ids, code pages, block id uniqueness, whole units |
| `text::tests_f12_b::…resource_header_keeps_every_line_and_raw_values` | every line kept, byte-exact reassembly, the four line shapes, per-define name/value/line/ranges, the raw-versus-trimmed value, an object-like define, `Missing`/`Ambiguous` |
| `…resource_header_values_are_typed_and_checked` | ten spellings including `0x`, `65536`, `-1`, `1+2`, a 20-digit value, `007`, `65535`, a bare `#define` and a tab-separated one; `MAX_RESOURCE_ID`; a trailing comment; an unclassified line; LF and a missing terminator |
| `…resource_header_is_bounded_by_the_allocation_budget` | the exact budget reads, one byte less is refused and books nothing |
| `…dialect_inventory_points_at_the_new_readers` | the two rows name their readers, no `VerifiedOriginal`, the deferred rows unchanged, routing through the observed rules, the shared id space and its two ranges |
| `…retail_resource_headers_read_within_the_id_space` (ignored, retail) | both members' lengths, CRLF only, nothing unclassified, every value a resource id inside the id space, unique names |
| `cs_content::config::tests::…negative_overflow_and_nan_never_become_tuning_constants` | **AC02**: negative, overflow (width, digit count, hexadecimal), `nan`/`inf`/exponent, a fractional value, a non-numeric field, blank padding, and that the raw bytes survive a refusal |
| `…approved_ranges_bound_tuning_values` | the approved range, its inclusive edges, a one-sided bound, a negative lower bound, and that an unbounded float is still checked for finiteness |
| `…tuning_converts_a_looked_up_entry` | the production path: `lookup` → `TuningSchema::tune`, the consumed accounting, an unsplittable value, a field index past the end |
| `…every_declared_width_rejects_its_own_overflow` | all four whole-number widths signed and unsigned against one value and against a negative, and each width's declared span |

The retail tests fail with "CS_GAME_DIR is not set" when run without it.

## Mutation probes

| Mutation | Failing tests |
| --- | --- |
| `Walker::walk` no longer checks whether `rel` is already on the path | `pe_resource_offsets_are_cycle_and_bounds_checked` (the self-referential case then terminates on `MAX_RESOURCE_DEPTH` instead) |
| `SectionSpan::span` drops its `file_offset > image_len` refusal | `pe_resource_offsets_are_cycle_and_bounds_checked` (the unmapped-RVA case) |
| `check_leaf` runs `check_string_block` on every leaf, not only a three-level `RT_STRING` one | `pe_resource_tree_keeps_ids_languages_and_code_pages` (the opaque six-byte payload) |
| `ResourceIdValue::resource_id` accepts any all-digit value of any length | `resource_header_values_are_typed_and_checked` (`65536`, the 20-digit value) |
| `TuningSchema::tune` drops the unsigned negativity check | `negative_overflow_and_nan_…`, `every_declared_width_rejects_its_own_overflow` |
| `TuningSchema::tune` drops the approved-range check | `approved_ranges_bound_tuning_values`, `tuning_converts_a_looked_up_entry` |

## Recorded unknowns

- **Whether the game reaches these strings through the Win32 resource API, its
  own `.H` headers, or both.** Nothing in the workspace measures this; the
  correlation between a `.H` id and a `RT_STRING` id is not established.
- **What type `255` and the `RT_VERSION` resource in `strings.dll` are.** The
  tree names them; nothing says what they are. Filed as a follow-up.
- **Whether a localized installation keeps the same block ids.** Only an
  English installation was surveyed (the shipped `langui.dll` is a *resource*
  image, not a localization of `strings.dll`; whether another language ships
  its own `strings.dll` is not established).
- **Fractional configuration values.** The reader accepts one decimal point
  (`Inferred`, a designed rule) because a tuning float needs one, but the
  survey found none. Filed as a follow-up with the F12-D measurement.
- **The original reading rules of the keyed field list dialect** (owner ruling
  2026-09-28, #351) still gate the configuration verification: a `;` after a
  value, trimming of values and fields, escaped quotes, case folding and the
  `<NAME>` placeholders the survey found are all still unmeasured. This stage
  reads fields, so it inherits them; `TuningSchema` trims the blank bytes the
  dialect's own rules define and nothing else.
- **The code page of non-ASCII bytes** in a localized installation.
- **A `Reader` with absolute-offset random access** (`io.rs`, a follow-up
  task), which would remove the module's private `Image` accessors.

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0
- `cargo test --workspace --locked -- accept_f12_b_ --include-ignored` → 0
  (15 tests: 11 in `cs_formats` of which 2 retail, 4 in `cs_content`)

## Sources

`specs/F12-text-configuration-strings-and-pe-resources.md`,
`docs/contracts/IDENTITY-CONTENT.md` (its "Numeric contract" section is the
rule this stage's typed values implement), the F12-A findings for the member
inventory and the keyed-list reader, the ROF and ZBD readers for the
allocation-ledger discipline, the Microsoft PE/COFF format specification
(`Documented`: `IMAGE_RESOURCE_DIRECTORY`, `IMAGE_RESOURCE_DATA_ENTRY`,
`RT_STRING`'s sixteen ids per block), and `$CS_GAME_DIR` (read-only) for the
surveys above.
