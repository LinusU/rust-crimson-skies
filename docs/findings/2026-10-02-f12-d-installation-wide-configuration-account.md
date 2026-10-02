# F12-D: the whole-installation account of referenced configuration fields and string ids

Date: 2026-10-02. Task: #482, split out of #48 (`F12-D`) because it does not
depend on a second, localized installation. Required capability: `retail`, used
**read-only**.

**Owner paths used:** `crates/cs_content/src/config.rs`,
`tools/cs_inspect/src/config.rs`, this file, and
`docs/findings/evidence/F12-D-ACCOUNTING.json` (the acceptance report). Wiring
in the two crates' `lib.rs`/`main.rs` was not needed: `cs_content::config` is
already a public module and `cs-inspect`'s `config` module already exports its
commands, so the only wiring was the `config-account` dispatch line and its
`--help` paragraph in `tools/cs_inspect/src/main.rs`.

Installation fingerprint, from the production `cs-inspect inventory` run whose
report is the evidence artifact:

| Fingerprint | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |

Nothing from the installation is committed beyond paths, byte spans, sizes,
SHA-256 digests, resource ids, language ids, counts and structure
descriptions. **No string text and no `#define` name is reproduced in this
file**; where a name would identify a row, the row is described by its id, its
count or its *name category* instead.

## What the task adds, and what was already there

The merged F12 work pinned each *member* on its own: #371 (`F12-I`) the record
kinds and their field kinds for `ASSETS/LAYOUT.CSV` and `ASSETS/SCRAPBOOK.CSV`;
#351 the reading rules R1-R4; #370 (`F12-E`) `<NAME>` placeholder expansion;
#368/#377 (`F12-G`, `F12-K`) the `.H` ↔ `langui.dll` id correlation; #372
(`T351`) case-insensitive name lookup.

What did not exist, and what this task adds, is a **whole-installation account
of what is referenced**:

1. **Referenced fields.** `ConfigDocument::member_account`
   (`crates/cs_content/src/config.rs`) rolls a whole routed keyed-list member
   up: every entry is attributed to the one declaration that covers it — a
   declared record schema, the F12-E placeholder pass, or *nothing* — and every
   declared field position of every declared schema gets its own row saying how
   many records reached it, how many values a declared kind describes, how many
   stay a **recorded unknown**, and how many the shipped bytes do not spell like
   the declared kind at all. The account carries the member's own
   `SourceSpan` and its declared criticality, and `Parity` turns an entry no
   declaration consumed into a parity blocker exactly as spec F12
   non-negotiable #5 requires. `spells_kind` — the check task #371 could only
   run inside a test — is now production code, because an off-kind value is
   invisible without it.
2. **Referenced ids.** `account_string_ids` crosses the ids the original
   *names* (the `IDS_`/`STR_`/`SB_` defines of `ASSETS/SCRIPTS/RESOURCE.H` and
   `ASSETS/SCRIPTS/RESRC1.H`, read by `cs_formats::text::resource_header`) with
   the ids the three string images actually *carry*, and reports the difference
   in **both** directions: the ids a header names whose block an image lacks
   (`absent`), and the blocks no header value names (`unnamed_blocks`). Task
   #368 measured that difference once by hand; this is the production
   derivation of the same numbers.
3. **A surface.** `cs-inspect config-account --cs-path <dir> [--out <file>]`
   emits the whole census as JSON. The member list comes from the dialect
   inventory, not from a hand-typed pair of names, so a member the inventory
   learns about is accounted by the next run. Exit `0` when no
   gameplay-critical entry is unconsumed, `3` when one is or a member could not
   be read, `2` invalid input, `4` no installation selected, `1` a runtime
   failure, per `docs/contracts/CLI-EVIDENCE.md`.

## The shipped keyed-list members

Both members are compressed inside `GOSDATA/ASSETS/crimson.rof`, so the census
reports **both** extents: the document's `SourceSpan` carries the member's
stored offset in the container with the *decoded* length and the *decoded*
digest — because those are the bytes a document was read from — and the stored
extent sits beside it as `"stored"`. Neither number is lost, and a span whose
length were the stored count would not describe the parsed bytes.

| member | container offset | stored | decoded | sha256 of the decoded member |
| --- | --- | --- | --- | --- |
| `ASSETS/LAYOUT.CSV` | 37 359 | 15 078 | 56 148 | `b50ea48bbe97ea09…` |
| `ASSETS/SCRAPBOOK.CSV` | 52 437 | 7 639 | 35 154 | `28b5144c54120f52…` |

### `ASSETS/LAYOUT.CSV`

| quantity | value |
| --- | --- |
| entries | **822** |
| covered by a declared record schema | **636** |
| covered by the placeholder pass (`V`/`G` definitions) | **186** (157 local, 29 global) |
| covered by nothing | **0** |
| `<NAME>` references / resolved / unresolved | 1 313 / 1 313 / **0** |
| unclassified lines | **1** (line 101, `no_separator`) |
| unsplit values | 0 |
| fields split out | 6 852 |
| gameplay-critical unconsumed entries | **0** |

Every entry of the shipped layout member is covered by a declaration, so no
parity blocker arises — but that is a statement about *entries*, not about
*fields*. Per record kind, with the position census's three columns that are
not zero:

| kind | records | fields | untyped (recorded unknown) | off-kind | unnamed positions reached |
| --- | --- | --- | --- | --- | --- |
| Button | 119 | 1 975 | **137** | 0 | 1 072 |
| Pane | 106 | 954 | 0 | 0 | 0 |
| Text | 297 | 2 673 | 0 | **4** | 0 |
| Edit Box | 4 | 44 | 0 | 0 | 0 |
| Movie | 6 | 54 | 0 | 0 | 6 |
| Text List | 16 | 144 | 0 | 0 | 0 |
| Scrolling Text | 8 | 104 | 0 | 0 | 8 |
| Dropdown List | 70 | 840 | 0 | 0 | 0 |
| Listbox | 6 | 60 | 0 | 0 | 0 |
| Slider | 4 | 52 | 0 | 0 | 0 |

The `SoundObject` kind is documented but absent from the shipped member, so it
contributes no row; the ten above are the whole census. The per-position
detail behind the two non-zero columns:

| schema | position | name | kind | reached | described | untyped | placeholders | empty | off-kind |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Button | 5 | — | unknown | 119 | 0 | **119** | 0 | 0 | 0 |
| Button | 6 | — | name | 119 | 46 | 0 | 0 | 73 | 0 |
| Button | 7 | — | integer | 119 | 84 | 0 | 0 | 35 | 0 |
| Button | 8 | — | integer | 119 | 74 | 0 | 0 | 45 | 0 |
| Button | 9 | — | unknown | 119 | 0 | **6** | 0 | 113 | 0 |
| Button | 10 | — | unknown | 119 | 0 | 0 | 6 | 113 | 0 |
| Button | 11 | — | unknown | 119 | 0 | **6** | 0 | 113 | 0 |
| Button | 12 | — | unknown | 119 | 0 | **6** | 0 | 113 | 0 |
| Button | 13 | — | integer | 119 | 13 | 0 | 0 | 106 | 0 |
| Button | 19 | — | unknown | 1 | 0 | 0 | 0 | 1 | 0 |
| Text | 7 | Color | color | 297 | 199 | 0 | 94 | 0 | **4** |
| Movie | 1 | — | path | 6 | 6 | 0 | 0 | 0 | 0 |
| Scrolling Text | 11 | — | unknown | 8 | 0 | 0 | 0 | 8 | 0 |

Three things in that table are **not new measurements**; they are the earlier
findings re-derived through production code, which is what makes the account
trustworthy:

* the 137 button unknowns (119 at position 5, 6 at each of 9, 11 and 12, and
  the 6 `<NAME>` placeholders at 10) are exactly the positions task #371's
  `BUTTON_SCHEMA` comment describes and counts;
* the 4 off-kind `Color` values are exactly the four records task #369
  (`F12-J`) measured as misspelling the `0x…` spelling;
* position 19 reaches one record and is spelled empty there, as
  `BUTTON_SCHEMA` records.

An off-kind count is a different statement from an untyped count and matters
differently: an untyped value is a **recorded unknown** the schema never
claimed anything about, while an off-kind value is one the schema *does* claim a
kind for and the shipped bytes contradict. The four colour values are the
latter, which is a statement about the declaration meeting misspelt data, not
about a missing kind.

### `ASSETS/SCRAPBOOK.CSV`

| quantity | value |
| --- | --- |
| entries | **461**, all `Mission_Spread_Item` |
| fields split out | **7 376** = 461 × 16 |
| covered by nothing | 0 |
| untyped (recorded unknown) | **1 377** |
| off-kind | 0 |
| unsplit values, unclassified lines | 0, 0 |

The three untyped positions are the ones `SCRAPBOOK_SCHEMA` names and does not
type: position 3 `ImageType` (455 values, 6 empty), position 10
`Left,Top,Right,Bottom` (461) and position 11 `Zoom` (461). The other
thirteen positions are typed and every observed value spells its declared kind.

## The referenced string ids

The two resource headers, decoded through the production ROF mount, resolve and
the production header reader:

| member | decoded bytes | sha256 | `#define`s | unclassified lines | distinct ids | undecoded values |
| --- | --- | --- | --- | --- | --- | --- |
| `ASSETS/SCRIPTS/RESOURCE.H` | 29 579 | `61ec23270fdf1dc4…` | 635 | 0 | 612 | **0** |
| `ASSETS/SCRIPTS/RESRC1.H` | 8 922 | `5d9c896d7532a022…` | 185 | 0 | 173 | **0** |

Every shipped `#define` value is a plain decimal inside the 16-bit id space, so
nothing is dropped as undecodable; the `undecoded` column is the account's
proof of that, and it is a column rather than an assumption because a value
that is not a decimal must never be allowed to coincide with an id.

Under the numbering this project reports (`id = (block - 1) * 16 + index`), a
named id lives in block `id / 16 + 1`. The crossing, per image:

| image | blocks | counted units | distinct ids named (both headers) | of which the image carries | absent ids | blocks no header names |
| --- | --- | --- | --- | --- | --- | --- |
| `strings.dll` | 112 | 1 792 | 782 | 123 | 659 | 98 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 3 | 48 | 782 | 2 | 780 | 2 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 101 | 1 616 | 782 | **775** | **7** | **18** |

Per header, against `langui.dll`:

| member | distinct ids | present | absent | string-name defines | string-name distinct | present | absent |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `RESOURCE.H` | 612 | 605 | 7 | 351 | 346 | 339 | 7 |
| `RESRC1.H` | 173 | 173 | 0 | 181 | 171 | 171 | 0 |

and the seven absent string-table ids of `RESOURCE.H` are **600, 620, 2 002,
2 050, 2 054, 3 510 and 3 540**; the eighteen `langui.dll` blocks no header
value names are **2, 3, 4, 5, 190, 195, 196, 197, 198, 200, 201, 202, 204, 205,
206, 217, 219 and 227**.

Both are **the numbers task #368 measured with an independent walk of the
resource directories, reproduced exactly** — including its 775/782 headline,
its seven misses and its eighteen unaddressed blocks. The 605 + 173 = 778
present ids and 7 absent ids are 785 rows, three of which (`10064`, `10065`,
`10143`) both headers name, so the union is the 775 the finding reports. That
agreement is the point: the account is a production derivation, and it can be
compared with a measurement that did not go through this engine's reader.

The `string_*` columns are a **second scope, not a filter**: `IDS_`/`STR_`/`SB_`
are the prefixes task #368 counted string-table names under, and the account
reports both the whole id set and that scope so a name category outside those
prefixes is never silently dropped. Against `strings.dll` the string-name scope
is 77 present and 269 absent, and against `language.dll` 0 present and 346
absent — the two headers describe `langui.dll`, not those images, and the
account says so with numbers rather than with a claim.

## Recorded unknowns

Each names the affected content and what would resolve it, per the owner
directive on follow-up limitations. **None of these is closed by this task: the
census names them, and naming is the deliverable.**

1. **The 1 377 untyped values of the two keyed-list members.** Affected
   content: the button positions 5, 9, 11 and 12 (137 values) and the
   scrapbook positions 3, 10 and 11 (1 377 values). These positions have a
   **documented name and no established kind**, so the value stays raw and is
   never converted. Resolved by: measuring what the original reader does with
   them, which is code inside the SafeDisc-packed engine image; nothing in the
   workspace can resolve it, and no localizable field accompanies it in the
   shipped data. Gates any `verified_original` claim about the UI layout's
   button geometry and the scrapbook's image placement and zoom.
2. **The four off-kind colour values.** Affected content: four live `T`
   records of the shipped layout member. The schema declares `Color` at that
   position and the data spells a letter `o` where the `0` of `0x…` belongs.
   Resolved by: the same unmeasured original reader; the recorded unknown is
   already stated in the `F12-H`/`F12-J` findings and the dialect inventory.
   The account makes it a standing count instead of a test-time observation.
3. **The 18 `langui.dll` blocks no header value names** (and the 2 of
   `strings.dll` that only some ids reach). Affected content: 288 units of
   `langui.dll`. A block can be reached by a name the headers do not carry, by
   a resource the `.rc` referenced but did not define, or by a second `.rc` that
   is not in the installation; this account reports the set and does not choose
   between the three. Resolved by: the `.rc` sources, which the installation
   does not contain, or an original-run capture.
4. **The seven string-table ids `RESOURCE.H` names that `langui.dll` has no
   block for.** Affected content: those seven ids. Whether they are dead
   defines, ids for another image, or evidence that the two headers do not
   belong to one `.rc` set is unmeasured. Inherited from #368 and still open.
5. **Whether a `V`/`G` definition or a one-letter record field identifies a
   definition.** The account does **not** restate that rule:
   `ConfigDocument::member_account` asks the F12-E pass's own definition table
   which entries are definitions, so the two cannot drift apart. The ambiguity
   the shipped data cannot settle between the two rules is recorded in
   `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md` and in the
   dialect inventory and is unchanged.
6. **The localized-installation acceptance case (AC04).** Not attempted: it
   needs a second, differently localized installation, which this machine does
   not have, and task #48 records the blocking evidence and the owner input
   that unblocks it. `parity_holds: true` in this report is a statement about
   the *unconsumed-entry* rule on one installation and is **not** an
   original-verified localization claim.
7. **Which `RT_STRING` numbering the original addresses strings with.** The
   account uses `cs_formats::string_id`'s `(block - 1) * 16 + index` because
   that is the numbering this project reports; the account does not settle the
   question, and #368's boundary measurement is evidence for #374, which owns
   it. A different numbering would change which ids the headers match.

## Tests

Ten tests, all under the task prefix `accept_f12_d_accounting_`, five in
`crates/cs_content/src/config.rs` and five in `tools/cs_inspect/src/config.rs`.
Every assertion is produced by production code — `ConfigDocument::read`,
`ConfigDocument::member_account`, `RecordView`, `spells_kind`,
`account_string_ids`, `StringCatalog::read`, `read_resource_header`, the ROF
mount/resolve/bounded member reader, `install::discover` — and each test fails
if the behaviour it pins is removed or changed.

| Test | What it pins |
| --- | --- |
| `accept_f12_d_accounting_member_account_attributes_every_entry_and_position` | over an authored member with two record kinds, a `V`/`G` pair, an undeclared entry and a misspelt colour: the account carries the document's own `SourceSpan`, dialect and declared parity; every entry gets exactly one declaration; the placeholder pass's own table decides what a definition is; the `Text` colour position types two of three records and counts the third as *off kind*; every position partitions `observed` into described + untyped + off-kind + placeholders + empty; a schema no record follows contributes no row and the rows follow `RecordSchema::ALL`; the one unconsumed entry is a gameplay-critical blocker |
| `accept_f12_d_accounting_a_non_gameplay_member_counts_without_blocking` | the same member under `Parity::NonGameplay`: the undeclared entry is still retained, still enumerable and still in the report, but `blocking() == 0` and `parity_holds()` — the two halves of non-negotiable #5 are separate decisions |
| `accept_f12_d_accounting_a_recorded_unknown_position_is_counted_per_value` | one authored `B` record aligned to the twenty declared positions: position 5's value is a recorded unknown with no off-kind count (an unknown kind claims nothing), the empty positions are counted apart from it, and position 6 is typed though it has no documented name |
| `accept_f12_d_accounting_string_ids_cross_names_and_blocks_both_ways` | an image with blocks 1, 3 and 5 against a header naming ids 0, 16, 32, 48, 900 plus one `0x`-prefixed value: present/absent in both directions, the undecoded value counted and kept out of both id sets, the `IDS_`/`STR_`/`SB_` scope as a second scope and not a filter, the unnamed blocks, and the union over two headers |
| `accept_f12_d_accounting_retail_census_matches_the_recorded_measurements` | retail: the installed members' 822/636/186 and 461/461, the ten record kinds' records/fields/untyped/off-kind, the button's five underdetermined positions individually, the text colour position's 199/94/4, the scrapbook's 1 377, the one unclassified line, and the string-id account of all three images against both headers including the seven absent ids and the eighteen unnamed blocks |
| `accept_f12_d_accounting_command_reports_the_whole_installation_census` | the command over a synthetic installation (an authored container with the four routed members and three authored images): exit 0, the installation's own fingerprints, both members with their provenance and entry/declaration counts, the per-position census, both headers with every define and its parsed id (and a non-decimal value with `id: null`), and all three images with their own block counts and absent-id sets |
| `accept_f12_d_accounting_command_exits_three_on_an_unconsumed_gameplay_key` | the same tree with one undeclared entry: exit 3, `parity.holds: false`, the unconsumed list naming the line, section and key, and the other member still accounted |
| `accept_f12_d_accounting_command_refuses_a_member_the_container_does_not_hold` | a container missing three routed members: exit 3, one diagnostic per missing member naming the inventory rule, and a refusal row in the report rather than a silently shorter census |
| `accept_f12_d_accounting_command_refuses_invalid_input_and_no_installation` | an unsupported flag, a flag without a value and an `--out` inside the installation all exit 2 with no report; no installation (and an exported but empty `CS_GAME_DIR`) exit 4 with a diagnostic naming both ways of selecting one |
| `accept_f12_d_accounting_retail_command_reports_the_installed_census` | retail, end to end through the command: exit 0 with `gameplay_critical_unconsumed: 0`, both installation fingerprints, the per-member numbers above, the two headers' lengths, and the three images' block/unit/named counts, the eighteen unnamed blocks and the seven absent ids |

Mutation checks, applied to the production code and reverted, with the tests
that caught each:

- counting a value at an unknown-kind position as `typed` instead of `untyped` →
  `accept_f12_d_accounting_a_recorded_unknown_position_is_counted_per_value`
  and the retail census fail on the button's 137 and the scrapbook's 1 377;
- `member_parity` answering `Parity::NonGameplay` unconditionally →
  `accept_f12_d_accounting_command_exits_three_on_an_unconsumed_gameplay_key`
  fails (the run would exit 0 with a gameplay-critical key unconsumed);
- `account_string_ids` dropping the `absent` direction →
  `accept_f12_d_accounting_string_ids_cross_names_and_blocks_both_ways` fails.

**What the pins do not cover:** they re-derive the recorded numbers from the
production readers over the original files; they do not re-run an independent
walk. The agreement with #368 is a cross-check on the account, not a
substitute for the walk that produced #368, and both are named above so a
reviewer can compare them.

Every retail number is a hash, a byte span, a size, a count, an id or a
structure field — never original text and never a `#define` name.

## Evidence

This task used `retail` (read-only access to the original installation), so
`docs/contracts/CLI-EVIDENCE.md` requires an acceptance report. It is
`docs/findings/evidence/F12-D-ACCOUNTING.json`, generated on this tree by
`private/evidence/F12-D-ACCOUNTING/harness.py` from real runs: the full
workspace test suite, the production `cs-inspect inventory` fingerprint, and
the production `cs-inspect config-account` census of the installation (whose
own exit code the harness requires to be 0). The report carries digests,
counts, lengths and exit codes only; the artifacts stay in
`private/evidence/F12-D-ACCOUNTING/`.

It is validated with `tools/validate_evidence.py` **without** `--require-pass`,
for the reason the F12-G and F12-K reports give and that reason still applying
here: the flag rejects a report that lists unresolved issues, and this task's
deliverable is largely that the seven limitations above stay recorded with the
content they affect. Deleting the `unknowns` to turn the flag green would be
the thing the contract forbids. The report's `claim` is `implemented`: a merge
awards `checked` at most, and nothing here observed the original game running.

The report's `candidate_tree` is the tree the recorded runs were made on
(`CS_CANDIDATE_TREE`, as `docs/contracts/CLI-EVIDENCE.md` requires), and the
harness refuses to write unless the working tree is clean, that tree exists,
and **nothing but a `docs/` path has changed between it and `HEAD`** — so a
report describing different code cannot be produced. The reviewer regenerates
the report on the rebased commit and compares it, per
[Commands](#commands).

## Commands

Run on this branch's candidate tree (the first four are this repo's required
checks; the last two are the report generation and validation):

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (2 436 passed, 0 failed, 239 ignored on the rebased candidate tree; 2 432/226 before the rebase brought in the upstream campaign stages) |
| `cargo test --workspace --locked -- accept_f12_d_accounting_ --include-ignored` | 0 (10 tests, 10 passed, 0 failed — 8 synthetic and 2 retail) |
| `env -u CS_GAME_DIR cargo test --workspace --locked --no-fail-fast -- accept_f12_d_accounting_ --include-ignored` | 101 — both retail tests fail loudly with `CS_GAME_DIR is not set` |
| `cs-inspect inventory --cs-path "$CS_GAME_DIR" --out <private>/inventory.json` | 0 |
| `cs-inspect config-account --cs-path "$CS_GAME_DIR" --out <private>/config-account.json` | 0 |
| `CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') CS_ACCEPT_EXIT_CODE=0 CS_TEST_LOG_EXIT=0 CS_NODATA_EXIT=101 python3 private/evidence/F12-D-ACCOUNTING/harness.py` | 0 |
| `python3 tools/validate_evidence.py private/evidence/F12-D-ACCOUNTING/acceptance.json --artifact-root private/evidence/F12-D-ACCOUNTING` | 0 (`structurally_valid: true`, 5 artifacts) |
| the same command **with** `--require-pass` | **3** (`Unresolved issues`) — expected: the seven recorded unknowns are the pinned properties, not failed assertions |

## What is not claimed

- No behaviour of the original game is claimed or implied. The original
  executable was not run; `retail` here means read access to the installation's
  files.
- `parity_holds: true` is a statement about the unconsumed-entry rule over one
  installation. It is **not** a localization claim (AC04 stays open), **not** a
  claim that the members are *understood* — the 1 514 recorded-unknown values
  and the four off-kind values are named, not resolved — and **not** a
  readiness or `verified_original` claim about the UI layout or the scrapbook.
- A gameplay-critical classification is a **declared** argument, not a
  measurement: the command declares `ASSETS/LAYOUT.CSV` and
  `ASSETS/SCRAPBOOK.CSV` gameplay-critical, and a member it has not classified
  is treated as gameplay-critical so a missing classification blocks rather than
  passes. Both declarations are argued in the command's own source comment from
  task #372's measurement and the IDENTITY-CONTENT contract's required catalog
  collections; nothing in the census infers it from a file name.
- The `IDS_`/`STR_`/`SB_` prefixes are `ObservedTool` from one English
  installation, and the account reports both scopes rather than treating them
  as a filter.
- The `string_id` numbering is used, not settled; see the seventh recorded
  unknown.

**Identities.** Implementer: `bunny-alpha-2` (Space Bunny Alpha), in this
session. Not yet reviewed: the Rally reviewer's identity and fresh-context
status are recorded at review time in the report's `review` field, which
regenerates with the report.

## Sources

`$CS_GAME_DIR` read-only (the two keyed-list members, the two `.H` members and
the three PE images, all through the production readers);
`specs/F12-text-configuration-strings-and-pe-resources.md` (non-negotiable #1
"extract grammar from real samples before implementing a parser", and #5
"unknown configuration keys are retained and counted; gameplay-critical
unconsumed keys block parity rather than being discarded");
`docs/contracts/IDENTITY-CONTENT.md` ("Required catalog collections": UI/font/
string resources and stunts/scrapbook rewards; "A catalog element … Collections
cannot exclude failed entries"); `docs/contracts/CLI-EVIDENCE.md` (exit codes,
the evidence-record minimum and the task-test-discovery rule);
`docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`
(the `.H` ↔ `langui.dll` correlation, the seven misses and the eighteen
unaddressed blocks this account reproduces);
`docs/findings/2026-09-29-f12-i-record-kind-schemas.md` (the record-kind
schemas and their recorded unknowns);
`docs/findings/2026-09-29-f12-j-letter-o-colour.md` (the four misspelt colour
values);
`docs/findings/2026-09-29-f12-e-name-placeholders.md` (the `V`/`G` definition
rule this account defers to);
`docs/findings/2026-09-29-t351-keyed-list-reading-rules.md` (rules R1-R4 and
the record-identification ambiguity);
`crates/cs_formats/src/pe_resources.rs` (`string_id`, `STRING_UNITS_PER_BLOCK`,
the `other_leaves` accounting) and
`crates/cs_formats/src/text/resource_header.rs`; the Microsoft PE/COFF
resource-directory layout (`Documented`).
