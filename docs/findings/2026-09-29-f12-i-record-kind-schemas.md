# F12-I: the record kinds of `LAYOUT.CSV` and `SCRAPBOOK.CSV` typed from their documented field lists

Date: 2026-09-29. Task: #371 "Type the configuration record kinds of
LAYOUT.CSV and SCRAPBOOK.CSV from their documented field lists" (key
`F12-I`). Shared contract:
[`docs/contracts/IDENTITY-CONTENT.md`](../contracts/IDENTITY-CONTENT.md).
Required capability: ordinary build/test. This machine also has `retail`, and
it was used **read-only**: the two members are decoded out of
`GOSDATA/ASSETS/crimson.rof` into `private/` (git-ignored) or read straight
from the archive by an `#[ignore = "requires CS_GAME_DIR"]` test. Nothing
from the installation is committed except paths, lengths, counts, hashes and
the field-name identifiers that are this task's deliverable. No line, prose
or value of the members' own comments is reproduced.

This stage builds on task #351's reading rules
([`2026-09-29-t351-keyed-list-reading-rules.md`](2026-09-29-t351-keyed-list-reading-rules.md))
and on the F12-E placeholder pass; it re-derives the record shapes from the
same two retail members so the `accept_f12_i_*` retail test can pin them.

## Files

- `crates/cs_formats/src/text/records.rs` (new): the documented metadata —
  `RecordKind`, `DocumentedField`, `documented_fields`,
  `documented_scrapbook_fields`, `optional_field_count`, the two comment
  notes. It reads no bytes and decides no type.
- `crates/cs_formats/src/text/tests_f12_i.rs` (new): the
  `accept_f12_i_*` tests for the documented lists.
- `crates/cs_formats/src/text/mod.rs`: module registration, re-exports and a
  doc bullet (wiring only).
- `crates/cs_content/src/config.rs`: the typed schema and its accounting —
  `FieldKind`, `RecordFieldSpec`, `RecordSchema`, `layout_schema`,
  `scrapbook_schema`, `FieldSpelling`, `RecordField`, `RecordView`,
  `RecordAccounting`, and the `accept_f12_i_*` tests.
- `docs/findings/2026-09-29-f12-i-record-kind-schemas.md` (this file).

**One observable failure:** the members' comments say which field each
position *is* but never what it *is* — no type, unit, signedness or range —
so a consumer cannot type any record field, and the underdetermined
positions would be silently dropped by a best-effort reader. The schema
declares a kind only where the shipped data measures one, and every field
whose kind the list and the data leave open stays in the document and is
counted by `RecordView::accounting` (non-negotiable #5). Nothing here
converts a value.

## What the comments establish — and do not

The eleven kinds of `ASSETS/LAYOUT.CSV` are selected by a one-letter first
field. The comment lines name each kind's field list; that is the whole
documented source.

| letter | kind | documented fields |
| --- | --- | --- |
| `B` | Button | `ID`, `ArtPath`, `X`, `Y`, `Z`, `TabOrder`, `ResID`, `HelpID`, `ScriptToExe`, `ScriptPri`, `EndScript?`, `Left`, `Top`, `Right`, `Bottom`, *(button-type enum)*, `Checked?`, `[ColorDisabled, ColorActive, ColorRollover, ColorDepressed]`, `Group` — 22 names, four bracketed optional |
| `P` | Pane | `ID`, `ArtPath`, `X`, `Y`, `Z`, `NUMFRAMES`, `IsRegion?`, `AlphaType`, `Volatile?`, `HelpID` — 10 |
| `T` | Text | `ID`, `ResID`, `X`, `Y`, `Z`, `Width`, `Height`, `Color`, `Justify` — 9 |
| `E` | EditBox | `ID`, `FontID`, `X`, `Y`, `Z`, `Width`, `Height`, `MaxChars`, `HelpID`, `TabOrder`, `TexTCOLOR`, `FrameColor`, `CursorColor` — 13 |
| `M` | Movie | `ID`, `X`, `Y`, `Z`, `ScaleX`, `ScaleY`, `# Loops`, `Region?`, `HelpID` — 9 |
| `A` | TextList | `ID`, `HelpID`, `X`, `Y`, `Z`, `Width`, `Height`, `TexTCOLOR`, `Justify`, `ItemSpacing` — 10 |
| `S` | Scrolling Text | `ID`, `BorderColor`, `BackColor`, `Slider`, `UpArrow`, `DownArrow`, `X`, `Y`, `Z`, `Width`, `Height`, `TabOrder`, `ResID`, `Color` — 14 |
| `D` | Dropdown List | `ID`, `Slider`, `UpArrow`, `DownArrow`, `DropUp`, `DropDown`, `ScriptPointer`, `X`, `Y`, `Z`, `Width`, `Height`, `TotalDisplayed`, `TabOrder` — 14 |
| `L` | Listbox | `ID`, `Slider`, `UpArrow`, `DownArrow`, `ScriptPointer`, `X`, `Y`, `Z`, `Width`, `Height`, `TotalDisplayed`, `TabOrder` — 12 |
| `Z` | Slider | `ID`, `X`, `Y`, `Z`, `MinVal`, `MaxVal`, `CurrVal`, `RegionFile`, `SliderFile`, `Left`, `Top`, `Right`, `Bottom` — 13 |
| `W` | Sound Object | `ID`, `WAVFileName`, `Channel`, `Volume`, `LoopCount (0=continuous)`, `Autostart?` — 6 |

`ASSETS/SCRAPBOOK.CSV` documents one list, named `Mission_Spread_Item`:
`Objective`, `ResourceID`, `ImageName`, `ImageType`, `X`, `Y`, `Alpha`,
`Width`, `Height`, `DrawOrder`, `Left,Top,Right,Bottom`, `Zoom`, `ZoomX`,
`ZoomY`, `TitleResID`, `TextResID` — **16**.

### The "seventeen"

The task record and the F12-A survey prose call the scrapbook list
"seventeen". The comment names sixteen, and the eleventh entry is a single
**quoted** field holding four comma-separated numbers (`"a,b,c,d"`); the
"seventeen" counts that group's four names as four fields. All 461 shipped
items have exactly sixteen fields, the eleventh quoted, so the data is
decisive and the schema uses sixteen.

### Two rules the data cannot separate

The member's first field of an object record is its kind letter, and a
`V`/`G` variable definition is a separate thing. Whether the original tells
the two apart by the key or by the first field *not* being a record letter
**is not established**: a definition's value may itself begin with a byte
that is a record letter, and the two candidate rules agree on every observed
line. `RecordSchema::for_entry` therefore excludes a `V<digits>`/`G<digits>`
key first, which is the rule that cannot misclassify a definition; the
ambiguity is the recorded unknown here.

## What the shipped data measures

Counts re-derived from the two decoded members (SHA-256 of the decoded
bytes: `LAYOUT.CSV`
`b50ea48bbe97ea098d76575115629bfbe6d0f55165fa364c939f9fb089892583`, 56 148
bytes; `SCRAPBOOK.CSV`
`28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1`, 35 154
bytes). `LAYOUT.CSV` has 822 entries: 186 `V`/`G` definitions and 636 object
records.

| kind | documented | shipped field counts | schema positions | fields counted unknown |
| --- | --- | --- | --- | --- |
| Button `B` | 22 | 15×65, 16×9, 19×44, 20×1 (119) | 20 | 596 |
| Pane `P` | 10 | 9×106 | 9 | 0 |
| Text `T` | 9 | 9×297 | 9 | 0 |
| EditBox `E` | 13 | 11×4 | 11 | 0 |
| Movie `M` | 9 | 9×6 | 9 | 0 |
| TextList `A` | 10 | 9×16 | 9 | 0 |
| Scrolling Text `S` | 14 | 13×8 | 13 | 8 |
| Dropdown `D` | 14 | 12×70 | 12 | 0 |
| Listbox `L` | 12 | 10×6 | 10 | 0 |
| Slider `Z` | 13 | 13×4 | 13 | 0 |
| Sound Object `W` | 6 | absent | 6 | (never observed) |
| `Mission_Spread_Item` | 16 | 16×461 | 16 | 0 |

Only `T`, `Z` and the scrapbook match their documented list position for
position. Where they do not, the divergence is the omission, insertion or
emptiness the shipped data forces:

- **`P`, `A`**: `HelpID` is not spelled; the remaining positions line up in
  order.
- **`E`**: `HelpID` and `TabOrder` are not spelled before the three colours.
- **`M`**: a file name is inserted at position 1, between `ID` and the
  coordinates, and `HelpID` is dropped. The inserted field measures as a
  path; the comment does not name it, so its name is a recorded unknown.
- **`S`**: position 11 is empty in every record; whether it is `TabOrder` or
  `ResID` (or neither) is not established. The trailing colour's position is.
- **`D`, `L`**: `ScriptPointer` and the trailing `TabOrder` are not spelled.
- **`W`**: the kind never appears in the member, so no field but the record
  letter has a measured kind; the documented names are still recorded.
- **`B`** is genuinely underdetermined: positions 9–12 and 19 are empty in
  every record; position 5 spells `0` in the records without colours and a
  name in those with them, so neither its name nor its kind is established;
  positions 6–8 and 13 measure numeric or name, 14 is the `Checked?` bool
  and 15–18 are the four optional colours. Five positions per record remain
  unknown (six in the 20-field record) and are retained and counted, never
  guessed.

## How evidence is marked

Each `RecordFieldSpec` carries a `ClaimStatus`:

- `Documented` — the documented list and the data agree position for
  position (`T`, `Z`, the scrapbook, and every record's `ID`).
- `Inferred` — the documented list minus the omission the data forces, so
  the name and position follow but the position is shifted (`P`, `A`, `E`,
  `M`, `S`, `D`, `L`, `B`'s named positions).
- `ObservedTool` — the kind is measured but the documented list does not
  name the field (`M` position 1, `B` positions 6–8 and 13).
- `Unknown` — neither a name nor a kind is established (`B` positions 5,
  9–12, 19; `S` position 11; every `W` field but the letter).

A `FieldSpelling` (`Empty`, `Placeholder`, `Integer`, `Hex`, `Color`, `Text`)
records what the bytes themselves show. It is a spelling, not a kind: a
`<NAME>` placeholder is resolved separately by the F12-E pass and may stand
for a value of any kind. Accounting counts a field as typed when its kind is
established, whatever its spelling, and as unknown otherwise; nothing is
converted.

## Checks

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo test --workspace --locked -- accept_f12_i_ --include-ignored`
  (5 tests: 2 in `cs_formats`, 3 in `cs_content`, one of them the retail
  read)
- Mutation checks: setting `B` position 5's kind to an integer fails the
  traceability, accounting and retail tests; making the definition-key rule
  match only `V` fails the classification test.
