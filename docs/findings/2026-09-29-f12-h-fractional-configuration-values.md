# F12-H: is a configuration value ever fractional?

Date: 2026-09-29. Task: #369 / F12-H "Establish whether configuration values
are ever fractional", a follow-up to F12-B. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md` (its "Numeric contract" section is the
rule the reading rules implement). Required capability: `retail`, used
**read-only**: both members were decoded through the production readers
(`cs_formats::read_tree` → `cs_formats::read_member` → `read_keyed_list` →
`ConfigDocument`), and cross-checked against a `cs-inspect rof --export-dir`
copy written into `private/` (git-ignored). Nothing from the installation is
committed except paths, lengths, counts, digests and value shapes.

This is not a lettered slice of the feature sheet: the spec's F12-A to F12-D
are already merged stages, and F12-H exists to settle one recorded unknown so
that F12-D's configuration verification is not gated on a guess.

## The question, and why it needed a measurement

F12-B shipped [`TuningSchema::number`](../crates/cs_content/src/config.rs) with
three accepted spellings. Two were `ObservedTool` — a signed decimal run
(`LAYOUT.CSV` and `SCRAPBOOK.CSV` both carry them) and a `0x` hexadecimal value
(`LAYOUT.CSV` carries `0x1000`, `0x1020`). The third, a single `.` with digits
on at least one side, was marked **`Inferred`**: a designed reading rule, kept
because a tuning float has to be spellable at all, with the F12-B survey's
negative result ("found no fractional value in either member") recorded as an
unknown.

A negative survey result is not a measurement of the thing the rule is about.
A `.` in a configuration member is common — it is how every texture and movie
file name is written — and F12-B's survey had counted *value shapes*, not
decided what a "value" is for this purpose. So the sentence that stood was
weaker than it looked, and nobody could tell from the workspace whether the
installed data needed the rule or merely tolerated it.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/config.rs`: the `TuningSchema::number` doc comment
  (the evidence paragraph for the `.` rule) and two new tests,
  `accept_f12_h_the_fractional_spelling_is_a_designed_rule` (authored) and
  `accept_f12_h_retail_configuration_values_are_never_fractional` (retail).
- `docs/findings/2026-09-29-f12-h-fractional-configuration-values.md` (this
  file), one forward pointer added to the F12-B finding, and
  `docs/findings/evidence/F12-H.json` (the acceptance report; see "Evidence").
- Wiring only: none. No module, re-export or dependency changed.

**The parser is unchanged.** The measurement did not find a shape to extend it
with, so branch (b) of the task applies and the code keeps the rule it has.

**One observable failure:** a reader that presented "no `.` byte in the member"
as "no fractional value" would pass a grep and fail the question, because
`LAYOUT.CSV`'s 232 `.` bytes are all real and none of them is a number. The
count that answers the question is the count of *field values that read as a
number*, and both numbers are asserted.

## Method

For each member, through production code only:

1. `cs_formats::read_tree` walks `GOSDATA/ASSETS/crimson.rof`; the member is
   located by its path segments and decoded by `cs_formats::read_member` under
   the default `RofLimits`.
2. `ConfigDocument::read` turns those bytes into owned nodes under the dialect
   the inventory routes the member to (`TextDialect::KeyedList`, never the
   `.CSV` extension).
3. Every field of every entry is taken as `RawField::value()` — the form the
   original reader hands a consumer (**R4**, blank bytes dropped) — and put to
   `TuningSchema::tune` with a `Float64` spec, which accepts any finite value
   the rule can read. The decoded member's own bytes are counted separately,
   so "no `.` in the member" and "no `.` in a value of it" are two different
   claims and both are measured.

A value that reads as a number and comes back with a non-zero `fract()` is a
fraction in the only sense this task cares about. The question is deliberately
**not** asked of a whole-number schema as well: `tune` refuses a fractional
value as an `Overflow` of the declared type rather than rounding it, so such a
reading could never report one and would only restate the first. (The retail
test was reviewed on exactly this point — see "Review notes" below.)

## Measurement

| Member | Bytes | Lines | Field values | Numeric (`Float64`) | Field values with a `.` | `.` bytes | Fractional values |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ASSETS/LAYOUT.CSV` | 56 148 | 1 222 | 7 272 | 3 664 | **229** | 232 | **0** |
| `ASSETS/SCRAPBOOK.CSV` | 35 154 | 500 | 7 376 | 5 098 | **0** | 0 | **0** |

`ASSETS/SCRAPBOOK.CSV` contains no `.` byte at all.

`ASSETS/LAYOUT.CSV` contains 232 `.` bytes. 229 of them sit in field values
(one per value; no value carries a second `.`) and the remaining 3 are the
ellipsis of line 101, `:For about box, these two are dummy objects, actual
x,y, not important...`, which the keyed-list grammar does not classify (its
first non-blank byte is `:`, so it is `Unclassified::NoSeparator` and yields
no entry at all).

### The shape of every dotted value

Every one of the 229 is a **file name**, at field index 1 of its entry
(221 values) or at indices 7 and 8 of the four slider entries
(`AP_S_MVOLUME`, `AP_S_EVOLUME`, `AP_S_VVOLUME` and `CP_S_MOUSE`, each naming
`PF_B_SliderSlot.png` and `PF_B_Slider.png`, so eight values): a stem, exactly
one `.`, and an alphabetic extension. 125 distinct names. The extension set,
case included:

| Extension | Values | Case-folded |
| --- | --- | --- |
| `png` | 186 | `png` (208) |
| `Png` | 22 | — |
| `jpg` | 13 | `jpg` |
| `MPG` | 6 | `mpg` |
| `tga` | 2 | `tga` |

So the answers to the task's own questions, one by one:

- **trailing digits?** No. No dotted value has a digit after its `.`; every
  extension is alphabetic.
- **sign?** No dotted value carries a sign. (A sign in front of a *decimal* is
  a separate rule with its own evidence and is not part of this question.)
- **exponent?** No value is spelled as one. The count of field values with a
  letter `e`/`E` immediately next to a digit is 115 in `LAYOUT.CSV` and 4 in
  `SCRAPBOOK.CSV`, and every one of them is a `0x` hexadecimal colour, a
  resource-id name or a `<NAME>` placeholder — see the misspelling below. Not
  one is a number, so an exponent is absent from the data rather than merely
  unobserved. The rule refuses an exponent as text in any case.
- **more than one `.`?** No, not inside a field value. The only three-dot run in
  either member is the `...` of the unclassified line 101.

Every dotted value is a file name, and a file name is not a number: the stem
before the `.` is not a run of digits, so `TuningSchema::number` refuses all
229 as `NotANumber` for a reason that has nothing to do with fractions. The
assertion in the retail test is exactly that — `!stem.bytes().all(is_ascii_digit)`
— so it would fail if any dotted value ever *were* a number, whatever came
after its point.

### A finding this survey did not go looking for

Lines 1156-1158 of `ASSETS/LAYOUT.CSV` spell a text colour **`oxff1E283C`** —
a letter `o` where the `0` of `0xff1E283C` belongs. The three entries are
`SBZ_T_TITLEJ`, `SBZ_T_CAPTIONJ` and `SBZ_T_TEXTJ`, all carrying the same value
at field index 7, and every neighbouring colour on lines 1154 and 1160 is a
well-formed `0x…`. So it is a repeated slip in the original data rather than a
spelling the data uses, and `TuningSchema::number` refuses it as
`NotANumber { len: 10 }` for the same reason it refuses a file name.

This is recorded, not repaired and not modelled. The original's own reader
evidently tolerates it (these are live UI text colours, so the value is read on
every scrapbook visit), but nothing in the workspace measures how: whether it
skips the `o`, parses case-insensitively as `sscanf`, or clamps is unknown, and
guessing would be exactly the "unrecorded compatibility assumption" spec F12
forbids. A consumer that needs this colour declares what it accepts, and the
F12-D stage is where the original's own tolerance gets measured. It is a
follow-up, not a parity blocker for this task: the affected content is one
colour on three scrapbook panes.

## Resulting evidence class: `Inferred`, and now a measured negative

The rule stays **`Inferred`**, for a sharper reason than before. It is not
merely "unobserved"; it is *measured absent from the installed data*, which is a
different and stronger statement, and it is recorded as such:

- The spelling is not promoted to `ObservedTool`. Nothing in the data uses it,
  so there is no observation to promote it from, and a designed rule that no
  original value needs cannot be evidence about the original.
- The spelling is not removed either. A `FieldSpec` of `ValueWidth::Float32` or
  `Float64` exists, so a fractional value must be *readable* or the typed
  conversion would be incomplete by construction; refusing the spelling would
  convert "no data uses it" into "it is invalid", which is a parity claim the
  data does not support.
- `crates/cs_formats`'s `LexicalFeature` is unchanged, and deliberately so: it
  lists features a survey *observed*, and no fractional feature was observed.
  Adding one would be the promotion this task declines to make. That file is
  outside this task's owner paths in any case.
- **The limitation that survives this task:** the rule is not parity evidence.
  Whether the original reader accepts a fractional spelling at all is a
  property of the original program, not of its data, and no reading of
  `crimson.rof` can settle it. F12-D is where that is measured; until then
  nothing may rest on `5.5` reading the way this reader reads it. This
  limitation names the affected content (every float-typed configuration field)
  and the resolving task (F12-D) and must not be dropped when this task is
  marked done.

## Test inventory (`accept_f12_h_*`)

| Test | Covers |
| --- | --- |
| `…the_fractional_spelling_is_a_designed_rule` | the three accepted shapes (`5.5`, `5.`, `.5`) against a float width; the first and third refused as `Overflow` for a whole-number width and the second accepted as `5`; a sign in front of a fraction; `5.5.5`, `.`, `1.5e3` and `0x1.8` refused as `NotANumber` with their lengths; a plain decimal and a hexadecimal value unaffected; every refusal leaving the member byte-identical |
| `…retail_configuration_values_are_never_fractional` (ignored, retail) | both members read through the ROF tree walk, the bounded member decoder, the keyed-list grammar and `ConfigDocument`; the per-member field-value, numeric, dotted and exponent-shaped counts; the member-wide `.` byte counts (232 and 0) beside them; `SCRAPBOOK.CSV` dotted empty; `LAYOUT.CSV` exactly 229 dotted with no dotted value readable as a number; no value either member converts to a non-whole number; one `.` per dotted value, alphabetic extension, non-numeric stem; the exact extension histogram; every exponent-shaped value a hexadecimal colour, a resource-id name or a `<NAME>` placeholder, and the three `oxff1E283C` misspellings named exactly |

The retail test fails with "CS_GAME_DIR is not set" when run without it.

## Mutation probes

| Mutation | Failing test |
| --- | --- |
| `number` refuses any `.` (the reading rule deleted) | `…the_fractional_spelling_is_a_designed_rule` (`5.5` becomes `NotANumber`) |
| `number` accepts a `.` inside a hexadecimal value (a second `.` admitted) | `…the_fractional_spelling_is_a_designed_rule` (`0x1.8` becomes a number) |
| `tune` truncates a fraction for a whole-number width instead of refusing it | `…the_fractional_spelling_is_a_designed_rule` (`5.5` would become `Tuning { value: 5.0, … }`) |
| `number` refuses the `0x` prefix (the hexadecimal rule removed) | `…retail_configuration_values_are_never_fractional` (`LAYOUT.CSV` numeric count 3 664 → 3 220) |
| the member-wide byte count looks for `;` instead of `.` (review probe) | `…retail_configuration_values_are_never_fractional` (`SCRAPBOOK.CSV` `.` bytes 0 → 25) |
| the exponent-shape classification demands upper case again, with the misspelling matched in full (review probe) | `…retail_configuration_values_are_never_fractional` (`MS_P_19_01_SwanWelcomeHome1` is unaccounted for) |

One earlier probe, dropping the **R4** blank-trimming of `RawField::value`,
changed nothing: the seven padded fields of `SCRAPBOOK.CSV` are resource-id
*names*, which were never numeric. Recorded because it bounds what this task's
tests claim — they pin the value shapes, not the padding rules.

The probes also bound the negative claims above: the 115 exponent-shaped values
of `LAYOUT.CSV` are 28 hexadecimal literals, 63 `<NAME>` placeholders, 21
resource-id names and the 3 misspellings, and the four of `SCRAPBOOK.CSV` are
all names, so the classification is now what proves that rather than a
permissive predicate. What these tests cannot detect is a value that is a
fraction without being spelled with a `.` — the rule admits no such spelling,
and that is a property of the rule, not of this survey.

## Review notes

Recorded because the reviewer of this branch is the same agent identity that
implemented it (`bunny-2/bunny-2`, Rally claim on #369), so per AGENTS.md this
is **not** independent evidence, and a merged task is `checked`, never
`verified_original`. The review re-derived every number in the table above
from the exported members with a parser written separately from the production
readers — byte and field counts, the dotted census with its extension
histogram and stem classification, the hexadecimal/decimal split of
444 + 3 220 = 3 664, the exponent census of 115 + 4, and the three
`oxff1E283C` lines — and found them all correct. It fixed three things the
implementer left:

1. the retail test's `rounded` list was unreachable — `tune` refuses a
   fractional value for a whole-number width as an `Overflow`, so no value
   could ever land in it. It is now a `fractional` list measured on the float
   conversion, which can in principle be non-empty, and the dead `Bits64` spec
   is gone with it.
2. the exponent-shape assertion excused **any** value containing a letter `o`,
   which is how four real `SCRAPBOOK.CSV` resource-id names were passing
   unclassified. The misspelling is now matched as the exact spelling
   `oxff1E283C` and the name predicate admits the lowercase those names use, so
   "nothing else" is a claim the test makes.
3. the `232` `.` bytes the production doc comment states were not asserted.
   Both members' byte counts are now pinned (232 and 0), which is also what
   keeps the difference between a `.` byte and a fractional *value* honest.

## Evidence

The contract requires an acceptance report of a task that uses a capability
besides build/test, and this one used `retail` (it reads the original
installation, read-only). It is `docs/findings/evidence/F12-H.json`, generated
on the reviewed tree `b266998f39ab26137943d67022962f2778e3cc7b` (commit
`d10f66a`, the commit before this report was added, as in every other report
here) by `private/evidence/F12-H/harness.py` from real runs: the task's own
selection with `--include-ignored` (2 tests, 2 passed, 0 failed, 0 ignored — the
retail test is in the selection, so it ran) and the production `cs-inspect
inventory` fingerprint plus two production `cs-inspect rof` decodes of
`ASSETS/LAYOUT.CSV` and `ASSETS/SCRAPBOOK.CSV`. The report carries digests,
counts and lengths only; the artifacts stay in `private/evidence/F12-H/`. The
harness refuses to write a report unless both expected tests ran and passed.

It is validated with `tools/validate_evidence.py` **without** `--require-pass`,
because that flag rejects a report that still lists unresolved issues, and this
task's acceptance criterion is precisely that the unestablished reading rules
stay recorded. Deleting the two `unknowns` to make the flag pass would be
exactly the thing the contract forbids: one is the original's tolerance of a
fraction (F12-D's measurement) and one is the `oxff1E283C` data slip (F12-J).

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0
- `cargo test --workspace --locked -- accept_f12_h_ --include-ignored` → 0
  (2 tests, 1 of them retail, in `cs_content`)

## Sources

`specs/F12-text-configuration-strings-and-pe-resources.md` (non-negotiable #1,
"extract grammar from real samples before implementing a parser", and #2,
"numeric conversion is typed and checked"),
`docs/contracts/IDENTITY-CONTENT.md` (the "Numeric contract" section and the
`ClaimStatus` vocabulary used above), the F12-A, F12-B, T351 and F12-E findings
for the member inventory, the keyed-list grammar and the reading rules R1-R4,
`cs_types::evidence::ClaimStatus`, and `$CS_GAME_DIR` (read-only) for the
measurement above.
