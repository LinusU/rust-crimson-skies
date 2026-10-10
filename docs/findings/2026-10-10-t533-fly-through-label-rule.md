# #533: the fly-through selector's label rule, decided and re-measured

Date: 2026-10-10. Task: #533 "Decide the fly-through selector's label rule and
re-measure the campaign stunt records" (key `T465-flythrough-selector`), filed
by #465. Feature sheet:
`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`, stage
`### F42-D` (the retail audit stage). Shared contracts:
`docs/contracts/CLI-EVIDENCE.md` (evidence) and
`docs/contracts/STATE-TRANSACTIONS.md` (the boundary this feeds).
Predecessors: `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`
(the encoding measurement) and
`docs/findings/2026-10-02-t465-ai-stunt-earning.md` (which found the
disagreement and filed this task).

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered and nothing is played.

**Nothing here is `verified_original`.** No original run happened. `retail`
here is read access to files; the objective, scenario and machine bytes are the
only evidence, and what the 2000 PC original *did* with the three disputed
records is not established. A different agent instance with a fresh context
should review this format/fidelity decision, and no agent review replaces the
owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the F02-B
`install::fingerprint` over the whole manifest).

## The question, stated so it can be answered

`cs_content::stunts::scenario_fly_through_targets` (#463's selector) read
**both** halves of the measured label pair: its `?` on `category_label` refused
a record that carried only `help_label = MSG_OBJ_FLYTHROUGH`. #465 found that
over the whole installation the selector and the looser union of the two labels
disagree on exactly **three** campaign-mission records, all carrying the help
label and no `category_label` at all:

| reader | objective description | node |
| --- | --- | --- |
| `ZBD/C1/M02` | `MSG_OBJ_ZEPHANGER` | `h3_marker` |
| `ZBD/C4/M03` | `MSG_TRGT_DEVILSHORN` | `dz2` |
| `ZBD/C5/M02` | `MSG_TRGT_PHQ` | `dz1` |

So the measured counts were **67** by either label and **64** by the strict
selector. The question this task answers: which rule does the reimplementation
use?

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/stunts.rs` (an F42 owner path): the selector
  `scenario_fly_through_targets` and its record `ScenarioFlyThroughTarget`
  (whose two label fields become `Option<String>`), the label predicate
  `is_fly_through_labelled`, and the survey records that carry both counts
  (`RetailObjectiveCorpus`, `RetailStuntAuthoritySurvey`,
  `RetailStuntGate`).
- `crates/cs_app/tests/accept_t533_flythrough_label_rule.rs` (new, F42 test
  path): the task's named tests, prefix `accept_t533_`.
- `crates/cs_app/tests/accept_f42_d_stunt_encoding.rs` (#463's tests) and
  `crates/cs_app/tests/accept_f42_d_ai_authority.rs` (#465's tests): the
  expectations the rule change moves, updated and re-measured.
- `crates/cs_app/tests/evidence_report_t533.rs` (new, F42 test path): the
  evidence harness; it is not part of the acceptance suite.
- `docs/findings/2026-10-10-t533-fly-through-label-rule.md` (this file) and
  `docs/findings/evidence/T533.json`.

**One observable failure:** a selector that required the category label is
indistinguishable, over the instant-action corpus, from the union rule — every
instant-action record carries both labels — so a test that only exercised
instant-action shapes could not tell the two rules apart, and the campaign
records the strict rule drops would vanish silently from the two
whole-installation consumers (the earning-authority survey and the
reward/repeat survey). The catalog's stunt rows and F18's opening audit read
only instant-action scenarios, so they were never affected — which is why the
instant-action measurements of both predecessors stand unchanged. That is why
the acceptance test authors a record with **only** the help label and a record
with **both** labels and asserts both are selected: inverting the rule to
require the category label drops the first and fails the test (verified: both
inversion directions — strict AND, and category-only — fail all three task
tests, as does a selector-only skip of category-less records).

## The decision: the union of the two labels

**The reimplementation selects a record when its `category_label` is
`MSG_OBJ_DZ` *or* its `help_label` is `MSG_OBJ_FLYTHROUGH`.** Measured
evidence, each item reproducible from the code in this branch over
$CS_GAME_DIR:

1. **The help label is the near-universal half of the pair.** Over the 332
   objective records in the 53 `targets.zrd` members, `help_label` appears in
   **294** records and `category_label` in only **146**. The category label is
   the optional half; requiring it is requiring the rarer key.
2. **Every labelled record carries the help label; only 64 carry both.** A
   from-scratch parse of every reader archive (written for this task,
   sharing no code with the workspace) measures **67** records carrying either
   label: **64** carry both, **3** carry only the help label, and **0** carry
   only the category label. The union adds exactly the three records above and
   nothing else.
3. **All 67 labelled records name a node**, so under the union rule the
   selector's count and the label-only reading agree at 67 — the two readings
   #465 kept side by side now measure the same corpus, and they would diverge
   only for a future labelled record that names no node (which the selector
   skips and the label-only count keeps, so the distinction stays honest).
4. **The three help-only records are wired into their missions like every
   both-label record.** Their nodes exist in their world containers, and their
   missions' objective machines reference them: `ADD_OBJECTIVE_TARGET`/
   `REMOVE_OBJECTIVE_TARGET` on `h3_marker` (C1/M02 `OBJECTIVE11`/`OBJECTIVE45`),
   on `dz2` (C4/M03 `OBJECTIVE2`/`OBJECTIVE46`) and on `dz1` (C5/M02
   `OBJECTIVE8`/`OBJECTIVE21`), and C5/M02's `OBJECTIVE15`/`OBJECTIVE20` carry
   `TRAVELERS ['player', 'APPROACHING', 'dz1', …]` conditions on the same node.
   For comparison, the both-label campaign records (C2/M03's nine, C3/M01's
   one) are wired the same way. Nothing in the data distinguishes the help-only
   records from their both-label siblings except the absent optional key.
5. **The instant-action corpus is unaffected.** All **54** labelled records of
   the eight instant-action scenarios carry both labels, so #463's encoding
   measurement (54 targets, 45 `stunt_flying`, the per-world table, every
   resolved box) is unchanged by this decision — verified by re-running #463's
   retail test, which passes unedited apart from the now-optional field types.
6. **The `c1c`/`c2b` `zeppelin_run` scenarios author no fly-through records at
   all** (0 of their 2 records each carry either label), so the looser rule
   pulls nothing into the worlds that author no stunts.

The fidelity argument in one sentence: the help label `MSG_OBJ_FLYTHROUGH` is
the original's own player-facing instruction for these objectives, the category
label is an optional grouping the campaign authors frequently omitted, and a
rule that required it would drop three original campaign fly-through objectives
— under-reporting original content is the fidelity loss, while the union loses
nothing the corpus contains.

What the decision is **not**: it is not a claim about the original's runtime.
Whether the original's engine awarded a stunt (fame, photo, score) for these
three campaign objectives is behaviour no file records; the campaign
`dzones.zrd` binding their labels to world nodes is still unmeasured (task
#513). The decision is about which records the reimplementation *counts as*
fly-through objectives: all of them, by the original's own labels.

## What changed, and what each number became

| number | was (#463/#465) | now | where |
| --- | ---: | ---: | --- |
| selector count, whole installation | 64 | **67** | `RetailStuntAuthoritySurvey::fly_through_objectives` |
| label-only count, whole installation | 67 | 67 (unchanged) | `…::fly_through_labelled_objectives` |
| instant-action targets (#463 survey) | 54 | **54** (unchanged) | `survey_retail_stunt_encoding` |
| `stunt_flying` targets (#463) | 45 | **45** (unchanged) | `…::stunt_flying_gates` |
| campaign/other labelled records | 10 strict / 13 union | **13** | per-reader census in `docs/findings/evidence/T533.json` |

The task description's estimate ("the campaign count moves from 13 to 16") was
wrong in both directions and the task said to verify rather than trust it: the
campaign/other labelled count was **10** under the strict rule and is **13**
under the chosen rule (10 both-label: C2/M03 ×9, C3/M01 ×1 — plus the three
help-only). The instant-action count stays **54**.

`ScenarioFlyThroughTarget.category_label` and `.help_label` (and
`RetailStuntGate`'s accessors) are now `Option<String>`: 3 selected records
carry no category label, and inventing an empty string for them would be
inventing data. The catalog's `stunt_rows` (F14) reads only instant-action
scenarios, so no catalog row moves; the F18 opening audit reads the encoding
survey, also instant-action-only, so no opening moves.

## Confirmations of the two predecessor findings

Every selector-dependent number in the two findings docs was re-measured:

* `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`: §3's
  **54** targets, **45** `stunt_flying`, the per-world table (`c1` 5, `c1b` 5,
  `c1c` 0, `c2` 9, `c2b` 0, `c3` 4, `c4` 14, `c5` 17) and §4's "all 54
  resolved" are **confirmed unchanged** — the rule change moves nothing in the
  instant-action corpus (evidence item 5 above; #463's retail test passes).
  One sentence there is now historical rather than current: the review section
  says "the target selector … is deliberately the union of `MSG_OBJ_DZ` and
  `MSG_OBJ_FLYTHROUGH`, and the retail corpus carries no target with only one
  of them" — true of the eight instant-action scenarios #463 surveyed, and
  superseded for the whole installation by this document (three records carry
  only the help label; the selector now unions the labels as #463's review
  prose already described).
* `docs/findings/2026-10-02-t465-ai-stunt-earning.md`: §1's **67** by either
  label, **332** records, **146**/**294** key counts, the **31**
  `DANGER_ZONES_COMPLETED` and **75** `TRAVELERS` blocks, the subject census
  and the team count are **confirmed unchanged**; the sentence "**67** … by
  either measured label and **64** by #463's stricter selector" is **corrected
  in meaning**: 64 was the strict selector's count and the strict rule no
  longer exists — the selector counts **67**, and the three records that made
  the difference are named in the table above. The doc's "Known limitations"
  bullet "The fly-through selector's label rule is undecided … Resolving task:
  #533" is **resolved by this document**.

Both documents keep their original measured numbers as records of what those
tasks measured with the rule that existed then; the corrections above are
appended, not substituted, and the affected records are named.

## Evidence

Ordinary build/test plus read-only `retail` access. The acceptance run's log
and a second production observation (the label rule per reader over the whole
installation, with the three help-only records named by container, description
and node) are recorded under `private/evidence/T533/`; the report is committed
as `docs/findings/evidence/T533.json` and checked with
`tools/validate_evidence.py --require-pass`. The claim is **`implemented`**: the
rule is decided and measured; the original's runtime treatment of the three
records remains unmeasured, as stated above.

## Known limitations that gate later stages (not silently dropped)

- **The original's runtime treatment of the three help-only records is
  unmeasured.** Affected content: C1/M02's `h3_marker`, C4/M03's `dz2` and
  C5/M02's `dz1` objectives. Resolving task: an original run (REF capture),
  which only the owner can supply; until then counting them as fly-through
  records is a data-level decision, never `verified_original`.
- **The campaign `dzones.zrd` semantics remain unmeasured** (`objective_numbers`,
  `disable`, `nosnapshot`), as both predecessors left them; task #513 owns that
  member's framing. The three records' label→node bindings are exactly what
  that member would carry for campaign missions.
- **No original run happened.** Nothing here is evidence of the original's
  runtime behaviour: not that a crossing of `h3_marker`/`dz2`/`dz1` completes
  anything, and not the traversal, payout or repeat rules (#464, F42-A).

## Test sensitivity (each mutation was applied, run and reverted)

| Removed behaviour | Tests that failed |
| --- | --- |
| the union rule, mutated to strict (both labels required in `is_fly_through_labelled`) | all three `accept_t533_flythrough_label_rule_*` tests |
| the union rule, mutated to category-only (help half dropped) | the same three |
| the selector specifically, mutated to skip category-less records again (the old `?` behaviour) while `is_fly_through_labelled` stayed the union | the same three |

## Checks run

* `cargo fmt --all -- --check` = 0,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0, `cargo test --workspace --locked` = 0 (4 372 passed, 0 failed, 782
  ignored), and
  `cargo test --workspace --locked -- accept_t533_ --include-ignored` = 0
  (3 discovered, 3 passed: 2 unignored for CI and 1
  `#[ignore] = "requires CS_GAME_DIR"]` run locally over `$CS_GAME_DIR`).
* #463's and #465's suites re-run over retail:
  `cargo test --workspace --locked -- accept_f42_d_ --include-ignored` = 0
  (17 discovered, 17 passed).

Review rerun (2026-10-10, same agent identity in a fresh review session, on the
branch rebased onto `origin/main` with the review's two documentation commits):
`cargo fmt --all -- --check` = 0,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
= 0, `cargo test --workspace --locked` = 0 (4 385 passed, 0 failed, 794
ignored), `cargo test --workspace --locked -- accept_t533_ --include-ignored` =
0 (3 discovered, 3 executed, 3 passed, the retail test included) and
`cargo test --workspace --locked -- accept_f42_d_ --include-ignored` = 0. The
review also mutated `is_fly_through_labelled` to the strict AND rule and watched
both unignored task tests fail before reverting, and corroborated the corpus
numbers over `$CS_GAME_DIR` independently with a byte-level occurrence scan of
every reader archive: **64** `MSG_OBJ_DZ` and **67** `MSG_OBJ_FLYTHROUGH`
occurrences, no archive carrying the category label without the help label, and
exactly `ZBD/C1/M02`, `ZBD/C4/M03` and `ZBD/C5/M02` carrying the help label
without the category. The committed evidence report was regenerated on the
reviewed tree and revalidated with `tools/validate_evidence.py --require-pass`.

## Sources used

- `specs/F42-stunts-fame-photos-and-optional-achievement-events.md` (F42-D) and
  the two predecessor findings documents (linked above).
- `crates/cs_formats/src/zbd/{trailer,reader_archive}.rs` and
  `crates/cs_formats/src/script_raw/discovery.rs` (the production reader-archive
  discovery), `docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`
  (the `.zrd` grammar).
- The owner's installation, read-only, over `$CS_GAME_DIR`: all 62 reader
  archives (`ZBD/**/zrdr.zbd`) and their `targets.zrd`, `objectives.zrd` and
  world-container members. Installation fingerprint above. The measurements were
  taken twice: once with a from-scratch parser written for this decision
  (`private/scratch/t533_*.py`, outside the repository) and once through the
  production survey (`survey_retail_stunt_authority`), which is what the
  committed evidence records.

**No original data is committed.** The numbers here are counts, offsets, spans
and digests; no extracted `.zrd`, no string table, no mesh and no screenshot is
in the repository, and every private output went to `private/`, outside it.
