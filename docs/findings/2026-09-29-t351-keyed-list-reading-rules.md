# T351: the original reading rules of the keyed field list dialect

Date: 2026-09-29. Task: #351 "Establish the original reading rules of the
keyed field list dialect (LAYOUT.CSV, SCRAPBOOK.CSV)" — the owner ruling of
2026-09-28 that gates F12's configuration verification. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Required capability: ordinary
build/test. This machine also has `retail`, and it was used **read-only**:
every member quoted below was decoded through `cs-inspect rof` into
`private/` (git-ignored) or read straight out of the archive by a test.
Nothing from the installation is committed except paths, lengths, counts,
hashes and the two identifier pairs named as evidence.

This stage is not a lettered F12 slice of the feature sheet: the spec's
`### F12-B` and `### F12-C` are already merged stages with a different
scope, and the owner resequenced the rest after this task was created. The
rules below are therefore cited by task number (#351) and by the follow-ups
#370 (F12-E) and #371 (F12-I).

F12-A surveyed the two members and listed what the survey could not settle.
This stage settles four of those points from the retail data itself and
records the rest as unknowns. **Nothing here was guessed**: every rule in
the table below either has a discriminating measurement or is marked
unknown.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/text/keyed_list.rs`: the module doc now carries
  R1-R4 and names what stayed unknown; `Field::value()` (new) is the field
  as the original hands it to a consumer (**R4**); `Field::text`,
  `KeyedListLine::indent`, `LineKind::Section`, `Entry::key` and
  `Entry::value` document the rules they already implement; a private
  `trim` helper.
- `crates/cs_content/src/config.rs`: `ConfigDocument::lookup` compares
  names without regard to ASCII case (**R1**) through a new private
  `names_match`; `RawField::value()` (**R4**); `TuningSchema::tune` reads
  `RawField::value` instead of trimming `text` itself; the module doc and
  `ConfigEntry::key` no longer call the fold unknown.
- `crates/cs_formats/src/text/dialect.rs`: the `KeyedList` row's `unknowns`
  list — the three entries that are now established are gone, the `@` around
  a section name (unknown 7, which the row never carried) is added, and the
  rest stay.
- `crates/cs_formats/src/text/tests_t351.rs` (new) and the two new tests in
  `config.rs`: the `accept_t351_*` tests.
- Wiring only: `crates/cs_formats/src/text/mod.rs` (`mod tests_t351;` and a
  doc paragraph), `crates/cs_formats/src/text/tests.rs` (`rof_entries` is
  `pub(crate)` so the new test module can reuse the retail walk).

**One observable failure:** a reader that keeps the blank bytes of a field
cannot resolve the resource-id names of `SCRAPBOOK.CSV` against the `.H`
headers — seven of them are written with trailing blanks and defined without
them — and a reader that compares names as exact bytes cannot bind a single
one of the 402 object names the UI scripts write. Both are pinned by the
`accept_t351_*` tests and were checked by mutation (below).

## The evidence that settles a name: the layout is read by the scripts

The `[@…@]` sections of `LAYOUT.CSV` are the UI descriptions of the 34
`.SCRIPT` members that drive them, and each script binds the objects of its
section **by name**. That makes the pair a closed, measurable system: a name
that resolves in the original is a name one file spells and the other
resolves.

Measured over the 34 scripts that own a section (the other 27 of the 61 bind
no object and have no section), reading both sides with the production
reader and the production member reader:

| Measurement | Value |
| --- | --- |
| `[@…@]` sections in `LAYOUT.CSV` | 34 (plus one unwrapped `[GLOBALVARS]`) |
| Section names a `.SCRIPT` base name matches, compared without regard to ASCII case | 34 (exactly one section per script, never two) |
| …of those, spelled **exactly** like the script's base name | **0** |
| Section headers the layout indents (10 tabs, 2 four-space) | 12 |
| Object names a script writes by hand (`.YC = "…"`) | 438 |
| …that resolve to an entry key of the section | 402 |
| …that resolve **case-insensitively** | **402** |
| …that resolve with exact bytes | **0** |
| …of the 402, on a key padded with blanks before its `=` | 271 |
| …of the 402, on an indented entry line | 145 |
| Names a script builds at run time (a prefix plus a counter, or a variable joined with a suffix) | 40 (35 prefixed, 5 joined) |
| …of those, matching a key case-insensitively | 40 (0 exactly) |
| Bound names no entry carries | 1 (one text object of the main menu) |
| Case-insensitive duplicate keys **within** one section | 0 (in both members) |

The last row is what makes the fold safe for this data: a name that answers
one lookup twice is refused as ambiguous, and the retail layout never has
one. Across sections the keys do repeat — `V1`…`V8` are section-local
variable definitions reused by most sections — but that is a different name
in a different section, which the lookup already scopes apart.

The exact-name row is measured on the section name *as the member writes it*
(`@MainMenu@`), against the script's base name (`MAINMENU`). The `@`
wrapper is dropped for the case-insensitive match only; whether the
original's own key carries the `@` is unknown 7 below.

Consequences, and only these:

- **R1 — a name is compared without regard to ASCII case.** 402 of 402
  hand-written object names and 34 of 34 section names resolve only when
  case is folded; none resolves exactly. There is no other way for the
  game's own UI to come up. The fold is ASCII case: the surveyed names are
  ASCII, and what the original does with a byte above `0x7F` is **not**
  established (recorded below), so the implementation folds ASCII only.
- **R2 — the blank bytes before a marker are not part of the name.** Twelve
  of the 35 section headers are indented and 145 of the 402 resolved object
  names sit on an indented entry line; the sections that are indented are
  among the ones whose scripts bind objects, so their names are read without
  the indent. A tab and four spaces are used interchangeably for the same
  nesting (ten tabs, two four-space indents, mixed *within* one section), so
  neither the width nor the character of the indent means anything. What the
  nesting *means* (a page inside a parent) is a data-model question, not a
  lexical one, and stays open (unknown 8).
- **R3 — the blank bytes between a key and its `=` are not part of it.** 271
  of the 402 resolved names sit on such a line, padded by one to sixteen
  blanks. The reader already trimmed the key; what is new is that the trim
  is not a convenience but the rule.

R1-R3 change no byte in the reader: the nodes, their ranges and
`reassemble()` are untouched. R1 changed one consumer,
`ConfigDocument::lookup`, which used to compare exact bytes and now folds
ASCII case (and still answers `Lookup::Ambiguous` when the fold makes two
entries collide — no section of either surveyed member spells one key twice
under a fold, so the retail data is unaffected).

## R4: the blank bytes around a field are dropped before it is used

`SCRAPBOOK.CSV` is the only member with quoted fields, and it is the only
one with padded ones:

| Measurement | Value |
| --- | --- |
| Entries | 461 |
| Quoted fields | 461 |
| Fields with blank bytes around them (all unquoted) | **7** |
| …of those, followed by a `,` rather than ending the value | 5 |
| …of those, the last field of their value | 2 |
| …that are resource-id names (`IDS_…`) | 7 |
| …defined byte for byte, without the blanks, in `RESOURCE.H` or `RESRC1.H` | **7** |
| Distinct `IDS_` names in the member | 165 |
| …of those, defined in a `.H` (146 of the 158 unpadded, 7 of the 7 padded) | 153 |

The seven padded fields carry a resource-id name; the same name, without the
blanks, is a `#define` in one of the two headers. A consumer that resolves
the name needs the padding gone, or those seven scrapbook items resolve to
no string. That is the same kind of necessity as R1-R3 — one file's spelling
has to satisfy the other's — and it is measured, not assumed.

So `Field::value()` and `RawField::value()` return the field without the
blank bytes around it, and `TuningSchema::tune` reads that instead of
trimming `text()` itself. `raw` and `text` keep the bytes, so
`reassemble()` is still byte-exact and the F12-A tests still hold.

Two things stay open about quoting, and the reader keeps refusing to guess
them: a field **with** blank bytes outside its quotes is never classified as
a quoted field at all (the reader keeps such a value whole as
`Fields::Unsplit` with `QuoteIssue::TextAfterClosingQuote` or
`QuoteIssue::QuoteInsideField`), and no surveyed field has a blank byte
*inside* its quotes, so whether such a blank belongs to the value is
**unknown**. `value()` keeps it — the conservative direction, since dropping
it would change a value the data spells exactly.

## Measured and recorded, but not implemented

**The `<NAME>` placeholders.** All 1313 references in `LAYOUT.CSV`
(113 distinct names) resolve; none is unresolved. A definition is an entry
whose key is `V<digits>` or `G<digits>` with exactly two fields,
`NAME,value` — 186 of them, 29 of them global in `[GLOBALVARS]`. A
reference resolves against the section-local definitions first (793
references) and the global ones otherwise (520). No local definition
shadows a global name, so precedence is unobserved, and every name and
reference is uppercase, so case folding of a placeholder name is unobserved.
`SCRAPBOOK.CSV` has no placeholder at all. The reader does not expand
placeholders: expansion is a second pass with a name table, a scope and its
own budget, and it belongs to the stage that wires the document into its
consumer (F12-C) or to the F12-D accounting. What is established here is the
*rule*, so that stage does not have to guess it.

**The record kinds.** The file documents itself: eleven comment lines name
the field list of each record kind (`B`utton, `P`ane, `T`ext, `E`ditBox,
`M`ovie, te`X`tList, `S`crolling text, `D`ropdown, `L`istbox, `Z`lider,
sound object), and `SCRAPBOOK.CSV`'s second line names its own seventeen
fields. `LAYOUT.CSV`'s 822 entries are 186 `V`/`G` definitions and 636
object records, of which ten kinds appear (the sound object does not) and
each record's first field is one of those ten letters. `SCRAPBOOK.CSV` has
no definition and no record letter: its 461 entries begin with a number.
The comments are a documented source for the *field lists*; they say nothing
about types, units or ranges, which stay F12-D's job. Whether the original
tells a variable definition from an object record by its `V`/`G` key or by
the first field not being a record letter cannot be told from the data,
because on every observed line the two rules agree.

**`0x` hexadecimal literals** are spelled `0x` 437 times and `0X` 7 times in
`LAYOUT.CSV`, so the prefix is case-insensitive; F12-B's value reader
already accepts both. Recorded, no change.

## Recorded unknowns

These stay unknown, are listed in the `KeyedList` inventory row, and are not
guessed anywhere in the code:

1. **A `;` after a value.** Not one of the 1283 entries in the two members
   contains a `;` at all, and in all 171 comment lines the `;` is the first
   non-blank byte. Whether the reader strips a trailing comment, and whether
   it accepts one after an *unquoted* field only or also after a quoted one,
   is unmeasured.
2. **Escaped or embedded quotes.** 461 quoted fields, all of the shape
   `"digits,comma"`; not one contains a `"`, and no entry of either member
   has a `""`. Whether `""` is an escape, and whether a quote may appear
   inside an unquoted field, is unmeasured.
3. **A line with no `=`.** Exactly one such line exists (line 101 of
   `LAYOUT.CSV`): a `:` followed by English prose, with no `=` and no `;`.
   `accept_t351_retail_object_names_resolve_only_case_insensitively` pins
   that the member has exactly one unclassified line, that the reader
   classifies it as `NoSeparator`, and that its first non-blank byte is `:`.
   What is *established* is only the negative. Every entry of
   `LAYOUT.CSV` is one of 186 `V`/`G` variable definitions or 636 object
   records whose first field is one of the ten record letters the file's own
   comments document; every entry of `SCRAPBOOK.CSV` (461 of them) has a
   number as its first field and no record letter at all. So a line with no
   `=` could only become a record whose first field is that whole line, and
   the shipped layout of a working game has no such object — which is why
   this reader yields no entry for it. Whether the original *skips* such a
   line silently, warns about it, counts it, or treats `:` as a second
   comment marker is **not** established, and no consumer may assume it.
4. **Bytes above `0x7F` in a name.** The installation is 7-bit ASCII
   throughout; a localized installation's code page, and whether the
   original folds case over those bytes, is unknown. The reader keeps names
   as bytes and folds ASCII only.
5. **A blank inside quotes** and **a blank at the start of a value**: never
   observed in either member (no field and no value begins with a blank), so
   both are open.
6. **Placeholder case and precedence** (above).
7. **The `@` around a section name.** 34 of 35 sections are wrapped in `@`
   and one is not; the reader reports the name as written, between the
   brackets, and whether the original's own key includes the `@` is not
   established.
8. **The nesting** the indentation shows, and whether the original looks a
   sub-section up by its bare name or by a path.
9. **The `;`-comment marker of the `.SCRIPT` and `.H` dialects** is not
   evidence for this one; the two are different readers.

## What the owner could observe, and how

Closing 1, 2, 3 and 7 needs the original game (`human_play`, which no agent
has). The cheapest observation is a *file*, not a screenshot: the game
rewrites a configuration file when preferences, controls or keys are saved
and when the game exits. The owner would

1. copy the installation to a writable folder, run the original from it,
   change one audio preference, one key binding and one control
   assignment, quit, and hand over the files the run rewrote with their
   names, sizes and hashes;
2. in developer mode, keep one setting that has no default (a value the
   shipped file never spells) so the written file shows a `;` after a value
   or a `""` inside a field if the writer ever emits one;
3. note what the game does with a value it cannot parse — whether a bad line
   in a configuration file is skipped, reported or fatal — by handing it one
   deliberately broken copy of a file and reporting what appears on screen.

Until then these nine points stay unknown, and no consumer may assume a rule
for them.

There is also a **code-side** lead that this session could not finish: the
engine image of the installation (`crimson.icd`, 2 580 578 bytes, a PE32 that
the installer disguises as a cursor) has an intact `.rdata` string section
but a `.text` and `.data` whose entropy is 7.9 and whose entry point is not
recognisable code; no known packer signature is present, and the member name
of the configuration file appears nowhere in the readable strings. Unpacking
it would turn rules 1-3 from "unmeasured" into code readings, but it is a
separate, larger unit of work and it is not a substitute for an observation
of the running game.

## Test inventory (`accept_t351_*`)

| Test | Covers |
| --- | --- |
| `cs_formats::text::tests_t351::…field_value_drops_surrounding_blanks` | **R4** on an authored member: padded fields of both kinds, a field of blanks only, a blank inside quotes kept, `raw`/`text` still verbatim, byte-exact reassembly, and the two quoting shapes a quoted field can never have (`TextAfterClosingQuote`, `QuoteInsideField`) staying whole |
| `…retail_object_names_resolve_only_case_insensitively` (ignored, retail) | **R1-R3** on the retail pair: 34 sections and exactly one per script, 0 exact section names, 402 hand-written object names with 0 exact matches, 271 padded keys, 145 indented lines, 40 run-time names, the 1 dangling name, no case-insensitive duplicate key within a section, and the member's single `NoSeparator` line — all read through `read_keyed_list` and `read_member` |
| `…retail_padded_fields_name_defined_resource_ids` (ignored, retail) | **R4** on the retail member: 461 quoted fields, exactly 7 padded unquoted fields, every one a `IDS_` name a `.H` member defines, read through `read_keyed_list` and `read_resource_header` |
| `cs_content::config::tests::…lookup_resolves_names_without_regard_to_case` | **R1** through the document: a folded section and key resolve, the entry keeps the bytes, two keys differing only in case answer `Ambiguous(2)` and consume nothing, a name differing in more than case is `Missing`, a byte above `0x7F` is not folded, and the consumed/unconsumed accounting follows |
| `…field_value_drops_surrounding_blanks` | **R4** through owned nodes: `RawField::value` against `text`/`raw`, a blank inside quotes kept, and a padded numeric field converting through `TuningSchema::tune` to the same constant an unpadded one would |

The retail tests fail with "CS_GAME_DIR is not set" when run without it.

### Reviewer corrections (bunny-1, review of #351)

The first submission's `exact_section_names` assertion compared the section
name — which the reader reports *between* the brackets, e.g. `@MainMenu@` —
against `[@MAINMENU@]`, brackets included. Those can never be equal, so
`assert_eq!(exact_section_names, 0, …)` was vacuous: it passed whatever the
retail data said. The comparison is now `@MAINMENU@` against `@MainMenu@`,
the test also refuses a base name the layout spells more than once, and a
mutation probe (always counting a match) makes the assertion report 34
instead of 0, so it now discriminates. The measurement itself is unchanged:
0 of 34.

The reviewer also re-derived every number in the two retail tables from the
decoded members and found them correct, pinned two claims that were prose
only (no case-insensitive duplicate key within a section; the single
unclassified line), removed an invented rule label `R5` that existed in a
doc comment but nowhere in the rules or this document, corrected the module
heading that called this stage `F12-B` (an already-merged stage with a
different scope), and added the `@`-wrapper unknown to the `KeyedList`
inventory row.

## Mutation probes

| Mutation | Failing tests |
| --- | --- |
| `ConfigDocument::lookup` compares `entry.key == key` again | `cs_content::…lookup_resolves_names_without_regard_to_case` |
| `names_match` compares the section name byte for byte | the same |
| `RawField::value` returns `text()` | both `cs_content::…` tests |
| `Field::value` returns `text` | `cs_formats::…field_value_drops_surrounding_blanks`, `…retail_padded_fields_name_defined_resource_ids` |
| the retail test's section-name match is always true | `…retail_object_names_resolve_only_case_insensitively` (34 against 0) |

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0
- `cargo test --workspace --locked -- accept_t351_ --include-ignored` → 0
  (5 tests: 3 in `cs_formats` of which 2 retail, 2 in `cs_content`)

## Sources

`specs/F12-text-configuration-strings-and-pe-resources.md` (non-negotiable
#1: extract grammar from real samples; #5: unknown keys are retained and
counted), `docs/contracts/IDENTITY-CONTENT.md`, the F12-A and F12-B
findings (the member inventory, the keyed-list reader, the typed values),
`$CS_GAME_DIR` read-only for every measurement above, and the F05-D/F12-B
precedent for reading a retail member through the production reader in a
test. No external source was used, and no independent reference to the
format was found: the sources in `docs/research/SOURCES.md` describe asset
extraction, the manual and community walkthroughs, none of which documents
how the game reads its own configuration.
