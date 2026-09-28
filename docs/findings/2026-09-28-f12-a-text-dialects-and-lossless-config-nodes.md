# F12-A: text dialect inventory and lossless configuration nodes

Date: 2026-09-28. Task: F12-A "Inventory dialects and define lossless
configuration nodes" (`specs/F12-text-configuration-strings-and-pe-resources.md`,
section `### F12-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Required capability: ordinary build/test. This machine also has `retail`,
and it was used **read-only** for the survey below (the spec's first rule:
"Extract grammar from real samples before implementing a parser"). Nothing
from the installation is committed except paths, lengths, hashes and
counts.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/text/mod.rs` (new): module doc, submodules,
  re-exports.
- `crates/cs_formats/src/text/lines.rs` (new): `LineTerminator`,
  `TextLine`, `TerminatorCounts`, `TextLines` (`content`, `raw`,
  `terminator_counts`, `reassemble`), `scan_lines` and the crate-internal
  `scan_in`.
- `crates/cs_formats/src/text/keyed_list.rs` (new): `KeyedList`,
  `KeyedListLine`, `LineKind`, `Unclassified`, `Entry`, `Fields`, `Field`,
  `QuoteIssue`, `read_keyed_list`.
- `crates/cs_formats/src/text/dialect.rs` (new): `TextDialect`,
  `MemberRule`, `ObservedEncoding`, `LexicalFeature`, `DialectReader`,
  `DialectRecord`, `TEXT_DIALECT_INVENTORY`, `dialect_for_member`.
- `crates/cs_formats/src/text/tests.rs` (new): the `accept_f12_a_*` format
  tests, two of them retail (`#[ignore = "requires CS_GAME_DIR"]`).
- `crates/cs_content/src/config.rs` (new): `ConfigDocument` (`read`,
  `source`, `dialect`, `nodes`, `entries`, `lookup`, `unconsumed`,
  `accounting`, `reassemble`), `ConfigNode`, `ConfigNodeKind`,
  `ConfigEntry`, `RawValue`, `RawField`, `Lookup`, `KeyAccounting`,
  `ConfigError`, plus the `accept_f12_a_config_*` tests.
- Wiring only: `crates/cs_formats/src/lib.rs` (`pub mod text;` and a doc
  paragraph), `crates/cs_content/src/lib.rs` (`pub mod config;` and a doc
  paragraph).

**Not created in this stage:** `crates/cs_formats/src/pe_resources.rs`
(the bounded, cycle-checked resource directory reader is F12-B's
deliverable; the PE images are inventory rows here) and
`tools/cs_inspect/src/config.rs` (an inspect command needs a mounted ROF
member to read, which waits for F05-C/F05-D; same reasoning F05-A and F06-A
recorded for their inspect commands).

**One observable failure:** a reader that splits values on every `,`
(quotes ignored) turns the quoted field `"1,2,3,4"` into four fields, and
`accept_f12_a_quoted_separators_stay_inside_one_field` fails; a scanner
that folds `\r\n` into `\n` breaks byte-exact reassembly and
`accept_f12_a_crlf_terminators_survive_and_reassemble` fails. Both were
checked by mutation (below).

## Survey: where the game keeps text

The installation has **no loose `.ini`, `.txt`, `.cfg` or `.csv` file**.
Its text lives inside `GOSDATA/ASSETS/crimson.rof`
(sha256 `acc9946874110e9741183384010bc02fd48923ae3d608fcf02a96498c3731174`).
A read-only walk of the archive's directory blocks (one block at a time
with `cs_formats::read_directory`, because `read_tree` refuses the retail
archive for overlapping extents — already recorded by F05-C for F05-D) found
847 members: 313 `.png`, 184 `.bm`, 143 `.tga`, 107 `.jpg`, 25 `.tif`,
8 `.wav` and these text members:

| Member (in `crimson.rof`) | Bytes | sha256 | Lines | Terminator |
| --- | --- | --- | --- | --- |
| `ASSETS/LAYOUT.CSV` | 56 148 | `b50ea48bbe97ea098d76575115629bfbe6d0f55165fa364c939f9fb089892583` | 1 222 | CRLF only |
| `ASSETS/SCRAPBOOK.CSV` | 35 154 | `28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1` | 500 | CRLF only |
| `ASSETS/SCRIPTS/*.SCRIPT` (61 members) | 273 056 total | — | — | CRLF only |
| `ASSETS/SCRIPTS/RESOURCE.H` | 29 579 | `61ec23270fdf1dc484085db93c936af4e4bb5bb177e3d8a3dd513a6bc4eefb78` | 646 | CRLF only |
| `ASSETS/SCRIPTS/RESRC1.H` | 8 922 | `5d9c896d7532a022a40c1e733eb5d21b7ca85219be4633e23ae69d88cb649c52` | 196 | CRLF only |
| `ASSETS/SCRIPTS/DEBUGINFO.TXT` | 20 505 | `8cedcc3d40edfe4e3abb801b69e0b959610938a86cbc48b91192633cae326613` | 1 260 | LF; last "line" is one NUL byte |

Every byte of every text member is 7-bit ASCII. `crimptch.rof` has one
member, `ASSETS/SCRIPTS/AIRFRAME.SCRIPT`, whose declared extent lies beyond
the 797-byte container (already recorded by F05-C for F05-D); it was not
read.

Loose files with text or string resources:

| File | Bytes | sha256 | Kind |
| --- | --- | --- | --- |
| `strings.dll` | 131 072 | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21` | PE (`MZ`) |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 32 768 | `2d7c904bb2d7b14aa03f5df38bb9df61da756f9263af57786b9d1da4e30f6072` | PE (`MZ`) |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 282 624 | `357e6bb05f1d2872a00e0976fdde44561cd5bb6a56d9f555d85d0ff1481faf49` | PE (`MZ`); `RESOURCE.H` says "Used by LangUI.rc" |
| `Readme.rtf`, `EULA.RTF` | 57 539, 266 123 | — | RTF (`{\rtf`) |

The PE images were only fingerprinted: no resource directory was parsed and
nothing was loaded.

## The dialect inventory

| Dialect (`code`) | Members | Grammar status | Reader |
| --- | --- | --- | --- |
| `text.keyed_list` | `LAYOUT.CSV`, `SCRAPBOOK.CSV` | `ObservedTool` | `read_keyed_list` (this stage) |
| `text.ui_script` | the 61 `ASSETS/SCRIPTS/*.SCRIPT` | `Unknown` | deferred to F13-A: a script language (blocks, typed locals, `callback(…)`, `switch`), not configuration |
| `text.resource_header` | `RESOURCE.H`, `RESRC1.H` | `ObservedTool` | deferred to F12-B: `#define NAME value` resource ids that pair with the PE images |
| `text.symbol_map` | `DEBUGINFO.TXT` | `ObservedTool` | deferred to F13-A: readable name → obfuscated script symbol |
| `pe.resources` | the three DLLs | `Documented` (public PE format) | deferred to F12-B |
| `text.rtf` | `Readme.rtf`, `EULA.RTF` | `Documented` | none: no game consumer is known |

Routing is by the observed member rule only. `ASSETS/OTHER.CSV`, a
`LAYOUT.CSV` in another archive or a loose `LAYOUT.CSV` have no dialect;
the `.SCRIPT` rule covers exactly the 61 members of that one directory
(asserted against the retail archive).

### What the keyed list dialect looks like (observed)

Measured over both members (content not reproduced):

- `[NAME]` section lines — 35 in `LAYOUT.CSV` (several indented with a tab
  or spaces, one with a trailing tab), one in `SCRAPBOOK.CSV`;
- whole-line `;` comments (171), some indented; `LAYOUT.CSV` documents its
  record formats in such comments;
- blank lines (233);
- `KEY=field,field,…` entries (1 283), with spaces or tabs between the key
  and `=` on 444 lines, empty fields on 204 lines, `<NAME>` placeholders
  and `0x…` values;
- double-quoted fields holding commas (`"0,0,0,0"`) on all 461
  `SCRAPBOOK.CSV` entries; no quote character appears in any
  `LAYOUT.CSV` entry;
- **not observed:** a `;` after a value, `""` or other escaped quotes, a
  quote inside a field, non-ASCII bytes, duplicate keys in a section, a
  line without terminator.
- One `LAYOUT.CSV` line starts with `:` and has no `=`.

It is neither CSV (no header row; `=` separates the key) nor a plain INI
(fields, quoting).

## Design decisions

- **Lines first, and lossless.** `scan_lines` records offset, content
  range and terminator per line; only `\n` ends a line, `\r\n` is `CrLf`,
  a lone `\r` is content, and a missing final terminator is `None`.
  `reassemble()` equals the input for every input, at the line, node and
  document layers.
- **Recognize only what was observed; keep the rest.**
  `read_keyed_list` never fails on content. A comment is `;` as first
  non-blank byte; a quote opens a field only as its first byte and must
  close before `,` or end of value. Every other quoting is
  `Fields::Unsplit { issue, at }` with the raw value kept; every line
  without `=`, with an empty key or with broken brackets is
  `LineKind::Unclassified { reason }`, kept and counted. The `:` line is
  one such line: whether the game skips it is unknown.
- **Nothing is trimmed away.** The key is the bytes before the first `=`
  without surrounding blanks (the key range and the line keep the
  padding); values and fields are verbatim, since whether the original
  reader trims them is unknown.
- **Bytes, not strings.** Keys, section names and values are `&[u8]` /
  `Vec<u8>`: the survey found ASCII only and the code page of a localized
  installation is unknown, so a Windows-1252 or UTF-8 name is neither
  rejected nor transcoded.
- **Budgets.** Every buffer the readers allocate is booked against the
  `ParseContext` allocation budget: the line and node tables, each
  entry's field vector in `read_keyed_list`, and in the content layer the
  owned node rows, their copied line bytes, the key, section, field and
  value copies and the consumed flags. A refused reservation is never
  charged and a failed attempt rolls its own charges back (F03-C retry
  contract); the keyed list parse that ran before a refused owned-node
  parse had succeeded and keeps its charges.
- **The content layer owns and accounts.** `ConfigDocument::read` takes a
  `SourceSpan` (IDENTITY-CONTENT) and the member bytes, routes through the
  inventory (`unknown_dialect` / `no_reader` otherwise), checks the length
  and builds owned nodes whose `offset` is relative to the member bytes.
  `lookup(section, key)` compares exact bytes (case folding by the original
  reader is unknown), returns `Ambiguous(n)` instead of choosing between
  duplicates, and marks what it returned as consumed; `accounting()` and
  `unconsumed()` report every entry nobody used (spec non-negotiable #5).
  Numeric conversion is not done here (F12-B, AC02).
- **Tests live inside the owner paths** (`src/text/tests.rs`,
  `config.rs`'s `mod tests`), and the fixtures are authored in them: they
  follow the observed lexical shapes but copy no original line.

## Test inventory (`accept_f12_a_*`)

| Test | Covers |
| --- | --- |
| `text::tests::…quoted_separators_stay_inside_one_field` | AC01 quoted separators; quoted `;`; empty fields and empty value |
| `…comments_blank_lines_and_sections_are_kept` | AC01 comments, blank lines, indented sections, key padding, section membership |
| `…crlf_terminators_survive_and_reassemble` | AC01 CRLF, LF, missing terminator, lone CR, offsets, byte-exact reassembly |
| `…non_ascii_names_survive_as_bytes` | AC01 Windows-1252 and UTF-8 keys, non-ASCII section names and values |
| `…unobserved_quoting_is_unsplit_not_guessed` | unterminated, text after closing quote, `""`, quote inside a field |
| `…unclassified_lines_are_kept_and_counted` | `:` line, empty key, malformed sections |
| `…node_tables_are_bounded_by_the_allocation_budget` | refusal with `AllocationBudgetExceeded`, nothing booked; the entry field vectors booked exactly (the tables' cost alone refuses `FIXTURE`, that cost plus the fields' bytes reads) |
| `…dialect_inventory_routes_only_observed_members` | routing by rule not extension; one row per dialect; no `VerifiedOriginal` |
| `…retail_keyed_lists_read_losslessly` (ignored, retail) | both retail members: recorded length, ASCII, CRLF only, byte-exact reassembly, only the `:` line unclassified, every value splits, quoted fields present |
| `…retail_text_inventory_matches_the_survey` (ignored, retail) | `.H` and `DEBUGINFO.TXT` lengths and terminators; the `.SCRIPT` rule covers exactly 61 members |
| `config::tests::…config_document_is_lossless` | AC01 through owned nodes; offsets, lines, terminators, non-ASCII lookup, provenance |
| `…config_document_counts_unconsumed_keys` | non-negotiable #5 accounting; duplicate key `Ambiguous`; exact-byte section compare; unsplit value kept |
| `…config_document_refuses_unrouted_members` | `unknown_dialect`, `no_reader` (F13-A), `length_mismatch`, `parse` |
| `…config_document_books_every_copy_it_owns` | every buffer the owned nodes allocate is booked exactly, on top of the keyed list parse; the exact budget reads and one byte less is refused |

The retail tests fail with "CS_GAME_DIR is not set" when run without it.

## Mutation probes

| Mutation | Failing tests |
| --- | --- |
| `split_fields` never treats `"` as opening a field | `quoted_separators_…`, `unobserved_quoting_…` |
| `split` never recognizes `\r\n` (CR stays in the content) | `crlf_terminators_…`, `comments_blank_lines_…`, `quoted_separators_…`, `non_ascii_names_…`, `unobserved_quoting_…`, `unclassified_lines_…` |

## Recorded unknowns

- Keyed list reading rules the survey cannot settle: a `;` after a value,
  trimming of values and fields, escaped quotes, whether indentation
  matters, how the `:` line is read, case folding of keys and sections,
  field types, units and `<NAME>` placeholder expansion. Filed as a
  follow-up task.
- The code page of non-ASCII bytes in a localized installation (only an
  English installation was surveyed).
- The `.SCRIPT` grammar and `DEBUGINFO.TXT`'s consumer (F13).
- The resource types, ids, languages and string code pages of the three PE
  images (F12-B).
- Whether the game reads the `.H` headers, `DEBUGINFO.TXT` or the RTF files
  at all.

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0
- `cargo test --workspace --locked -- accept_f12_a_ --include-ignored` → 0
  (14 tests: 10 in `cs_formats`, 4 in `cs_content`)

## Sources

`specs/F12-text-configuration-strings-and-pe-resources.md`,
`docs/contracts/IDENTITY-CONTENT.md`, the F03, F05-B, F05-C and F06-A
findings for precedent, and `$CS_GAME_DIR` (read-only) for the survey.
