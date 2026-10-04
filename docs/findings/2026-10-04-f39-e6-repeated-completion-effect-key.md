# F39-E6: is a repeated completion-effect key in one block meaningful?

Date: 2026-10-04. Task: F39-E6 "Measure whether a repeated completion-effect key
in one block is meaningful" (Rally #602; the sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has no
`### F39-E6` section, so the task-test prefix this stage uses is
`accept_f39_e6_`, stated here as the sheet states a prefix per stage). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F39-E6/acceptance.json`, committed as
`docs/findings/evidence/F39-E6.json`.

Question inherited from F39-E2 (unknown #6): when one `OBJECTIVE<N>` block spells
the **same** completion-effect key twice — `WAKE_OBJECTIVE_WHEN_I_COMPLETE` twice
in one block, say — is that meaningful? F39-E2 measured precedence between
*different* effects and had to choose a reading for this shape anyway: it counted
a repeated key as two sites but one effect. That was a reading, not a measured
rule, and this stage does not keep it.

## Files and the one observable failure (listed before editing)

* `crates/cs_content/src/objectives.rs`: `MeasuredRepeatedEffect` (new),
  `MeasuredBranchPrecedence::repeated_effects` (new field) with
  `needs_unmeasured_repeated_effect`, `repeated_effect_blocks` and
  `unmeasured_repeated_effect_reason` (new accessors),
  `UNMEASURED_REPEATED_EFFECT_KEY` (new named verdict),
  `ObjectivesSchemaError::RepeatedCompletionEffect` (new refusal) and
  `check_completion_effects` (the check), plus the module doc's F39-E6 section.
* `crates/cs_app/src/objectives.rs`: `measure_block_precedence` records the
  repeat per (block, kind) with every site in authored order; `RetailRepeatedEffect`,
  `RetailObjectiveRow::repeated_effects`,
  `RetailObjectiveCensus::{repeated_effects, repeated_effect_blocks,
  needs_unmeasured_repeated_effect, unmeasured_repeated_effect_reason}`; and the
  excluded-corpus survey `survey_excluded_objective_records` with
  `ExcludedObjectiveScope`, `ExcludedTargetsReading`, `ExcludedObjectiveRow` and
  `ExcludedObjectiveCensus`.
* `crates/cs_app/tests/accept_f39_e6_repeated_effect_key.rs` (new): 7 tests plus
  1 retail test, prefix `accept_f39_e6_`.
* `crates/cs_app/tests/evidence_report_f39_e6.rs` (new): the evidence harness.
* `crates/cs_app/tests/accept_f39_e2_block_precedence.rs`: the repeated-key test
  now asserts the shape is *measured* as a repeat beside "not a precedence
  question", and the `MeasuredBranchPrecedence` literal gains the new field.
* Wiring only: none — every edited file is an owner path.

**One observable failure.** Before this stage, `measure_block_precedence`
counted a block spelling `WAKE` twice as `effect_sites: 2` and
`multi_effect_blocks` unchanged, i.e. "two sites, one effect" — a silently
deduplicated reading, which is exactly what the task forbids: an importer
recovering such a record would see an ordinary single-effect declaration and
never be told the case is unresolved. On the declared side,
`DeclaredObjectiveProgram::try_new` accepted `[Wake → 2, Wake → 2]` on one
objective without comment, so the lowered runtime would receive the residue of a
repeated site with no name for what it was.

## The measurement, and its denominators

Two production surveys, both read-only, both failing rather than skipping a
member that cannot be located or decoded:

* `survey_retail_objective_records` — the mission corpus F39-D/E2 already
  measure, re-read for the repeat: **53** mission-scoped reader archives,
  **1338** `OBJECTIVE<N>` blocks.
* `survey_excluded_objective_records` (new) — the corpus the mission census
  excludes: every member of every `zrdr.zbd` archive `mission_scope` does not
  name, and every `targets.zrd` member of every reader archive.

| measurement | value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| reader archives outside mission scope | **9** (`zbd/zrdr.zbd` shared + `zbd/{c1,c1b,c1c,c2,c2b,c3,c4,c5}/zrdr.zbd` world-group) |
| members decoded and measured there | **612** (all members; 0 decode failures) |
| `targets.zrd` members measured | **53** (52 mission archives + the `zbd/c1c` world-group member) |
| `targets.zrd` objective records | **332** |
| mission blocks spelling a completion-effect key twice | **0** |
| excluded members spelling any completion-effect key, anywhere in the tree | **0** |
| excluded members spelling `TICK_DEPENDS_ON_OBJ` anywhere | **0** |
| excluded members declaring an `OBJECTIVE<N>` block | **0** |
| `targets.zrd` records carrying a branching key | **0** |

Two scopes can overlap: `zbd/c1c/zrdr.zbd`'s `targets.zrd` is counted both as a
member of an excluded archive and as a targets record, so the census reports
664 member rows (612 + 52). The spelling scan counts a key **anywhere** in a
member's decoded tree, not only in flat-field position — a `0` is the strong
measurement "the member does not even spell the key", which also covers records
of a shape `OBJECTIVE<N>` blocks would not appear in. Every `targets.zrd`
record's complete key vocabulary is the six-key target/stunt set
(`category_label`, `description`, `help_label`, `nodes`, `objective`,
`other_target`) — no branching key occurs.

## What the corpus establishes, and what it cannot

**Established.** The repeated-key shape is written **nowhere** in the readable
corpus: not in 1338 mission blocks, not in the 612 members of the archives the
mission census excludes, not in the 332 `targets.zrd` records. The measurement
is per-member and per-block, taken with the same production `.zrd` reader and
the same `measure_block_precedence` the mission census runs, so a `0` is a
count over a closed denominator, not a spot check.

**Not established.** What the original would *do* with a repeated key. "The
corpus never writes it" bounds the corpus; it is not evidence that the original
engine refuses, ignores, replaces or stacks a second site. The compiled program
behind `objectives.zrd` is not decoded (F13-B/C and F38 own the mission-language
instruction table), and no original executable has been run — `retail` is file
access, not behaviour. Other container families (`interp`, texture, sound,
animation, `gamez` archives) are outside this corpus's scope: the measurement
covers the reader archives and the `targets.zrd` records, which is where an
objective record could live.

**What would settle it.** A decoded compiled mission program (the instruction
table F13-B/C/F38 own), or an original run supplied through the owner —
`retail` capability alone cannot produce either.

## The named unknown, and the refusal that is not silent

* `UNMEASURED_REPEATED_EFFECT_KEY` is the verdict, stated once beside
  `UNMEASURED_BLOCK_PRECEDENCE` so the measurement, the record and this finding
  say the same thing. The two are deliberately different verdicts: a
  *conflict* is two different effect kinds sharing one objective (which of them
  applies?); a *repeat* is one kind declared twice in one block (what does the
  second site do?). A block can carry both shapes and keeps both readings.
* `measure_block_precedence` records the shape as
  `MeasuredRepeatedEffect { block, kind, sites }` — every site of the kind, in
  authored order, with its own target list and argument list, so a repeat whose
  sites spell different targets or different `NAP` numbers keeps both
  spellings. `needs_unmeasured_repeated_effect()` /
  `repeated_effect_blocks()` / `unmeasured_repeated_effect_reason()` are the
  query surface, per record (`MeasuredBranchPrecedence`), per mission row
  (`RetailObjectiveRow::repeated_effects`) and corpus-wide
  (`RetailObjectiveCensus` / `ExcludedObjectiveCensus`).
* The declared vocabulary refuses the residue by name:
  `ObjectivesSchemaError::RepeatedCompletionEffect` fires when **one** objective
  declares the same `(kind, objective)` pair twice — the only declared shape
  that can only have come from a repeated site. Deliberately narrow: one kind
  naming two *different* objectives stays legal (it is a multi-target site's
  residue, indistinguishable from `WAKE [2,3]`), and the same pair declared by
  two *different* objectives stays legal (they agree on what happens). F39-E5's
  `AmbiguousCompletionEffect` is unchanged and still refuses the
  different-effects-one-target shape; the two refusals do not overlap.

## Test inventory (`accept_f39_e6_*`)

`crates/cs_app/tests/accept_f39_e6_repeated_effect_key.rs` (7 unignored + 1
retail):

| Test | Covers |
| --- | --- |
| `a_repeated_key_is_its_own_unresolved_shape` | a `WAKE`/`WAKE` block: 2 sites, 1 repeat, both targets kept in authored order, the named verdict, and none of the multi-effect/conflict counters move |
| `a_repeat_and_a_conflict_are_two_questions` | `WAKE`, `WAKE`, `NAP` sharing a target: the conflict is measured *and* the repeat is measured, under two different verdicts |
| `a_repeat_keeps_both_sites_spellings` | two `NAP` sites with different numbers: both arguments survive verbatim — the unmeasured choice is data, not deduplicated |
| `a_record_with_no_repeat_reports_none` | the control: no repeat → empty list, 0 blocks, `None` reason |
| `the_measured_record_carries_the_repeat` | `RetailObjectiveRow::measured()` hands the repeat to `MeasuredObjectiveRecord`, so an importer sees the unresolved shape |
| `the_declared_form_refuses_a_repeat_by_name` | `RepeatedCompletionEffect` for `Wake → 2` twice and for `Nap → 3` with different numbers; the legal neighbours stay legal (same kind, different targets; same pair, different objectives); `AmbiguousCompletionEffect` unchanged |
| `the_survey_uses_the_measured_vocabulary` | the census's key list is the measured vocabulary itself; a synthetic member spelling a repeat is caught by the block walk |
| `retail_records_spell_no_repeated_effect_key` (`#[ignore]`, `CS_GAME_DIR`) | the whole table above over the owner's installation, with per-row invariants |

## Measured sensitivity

* Repeat detection removed from `measure_block_precedence` →
  `a_repeated_key_is_its_own_unresolved_shape`,
  `a_repeat_and_a_conflict_are_two_questions`,
  `the_measured_record_carries_the_repeat` and
  `the_survey_uses_the_measured_vocabulary` all fail.
* `repeated_effects` folded into `conflicts` → `a_repeated_key_is_its_own_unresolved_shape`
  fails on `conflicts.is_empty()`.
* The declared check keyed on kind alone (refusing `Wake → 2` + `Wake → 3`) →
  `the_declared_form_refuses_a_repeat_by_name` fails on the legal multi-target
  residue.
* The excluded survey silently skipping unscoped members → the retail test
  fails on the 612/53/332 denominators.
* `survey_excluded_objective_records` counting only `targets.zrd` → the retail
  test fails on `members_in_scope(OutsideMissionScope) == 612`.

## Unknown / deferred (not guessed)

1. **What a second site of one completion-effect key does.** Unmeasured
   (`UNMEASURED_REPEATED_EFFECT_KEY`); settled by the compiled mission program
   or an original run. The named unknown survives the zero count deliberately:
   the corpus's silence is a bound, not a rule.
2. **The runtime-side refusal.** `cs_sim::objectives::runtime`'s
   `check_uncontested_targets` still accepts a duplicated
   `(kind, target)` inside one `ObjectiveSpec` — the same residue the schema
   now refuses. No lowered program can carry it (the schema gate is the only
   way in), but an `ObjectiveSpec` built by hand bypasses it; filed as a
   follow-up rather than edited here, because `cs_sim` is outside this task's
   owner paths.
3. **`WAKE [2,2]` inside one site** — a repeated *target* in one site's list,
   a third shape beside a repeated key and a conflict — is equally unmeasured
   (the corpus spells no duplicate in a target list either; left as the
   `targets` data `MeasuredBranchSite` carries).
4. **The container families outside the reader archives** (`interp.zbd`,
   texture/sound/animation/gamez containers) are outside this corpus's scope;
   an objective record has only ever been measured inside a reader archive.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e6_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e6 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E6/acceptance.json \
  --artifact-root private/evidence/F39-E6 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the F39-D
findings (unknowns #3, #5), the F39-E2 findings (unknown #6 — the shape this
stage measures), the F39-E5 findings (the declared vocabulary the refusal joins),
the F13-B/C and F38 findings (the undecoded compiled program), the F42-D/t463
findings (the `.zrd` reader both surveys reuse), and the read-only `$CS_GAME_DIR`
listing. No web source was consulted and no original executable was run.
