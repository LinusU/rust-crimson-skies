# F39-E2: the precedence between `WAKE`, `NAP` and `KILL` in one objective block

> **Correction (F39-D-COUNT, 2026-10-05):** the block counts below that come from the flat walk undercount `BEGIN_DORMANT` (1096 → 1118; sentinel 992 → 1014). See `2026-10-05-f39-d-count-bare-directive-walk.md`.

Date: 2026-10-03. Task: F39-E2 "Measure the precedence between WAKE, NAP and KILL
in one objective block" (Rally #596; the sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has no
`### F39-E2` section, so the task-test prefix this stage uses is
`accept_f39_e2_`, stated here as the sheet states a prefix per stage). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(read-only `$CS_GAME_DIR`). Evidence report: `private/evidence/F39-E2/acceptance.json`,
committed as `docs/findings/evidence/F39-E2.json`.

Question inherited from F39-D (unknown #3): F39-D counted the branching
declarations per mission — how often the original writes `WAKE…`, `NAP…`,
`KILL…` — and left the per-block case open. What happens when **one block
declares more than one of them for the same event**, and in which order do they
take effect?

## Files and the one observable failure (listed before editing)

* `crates/cs_content/src/objectives.rs`: `BRANCH_EFFECT_KEY_VOCABULARY`,
  `BRANCH_ORDER_KEY`, `BRANCH_EFFECT_MULTI_TARGET_KEYS`, `BranchEffectKind`,
  `MeasuredBranchSite`, `MeasuredBranchConflict`, `MeasuredBranchPrecedence`,
  `UNMEASURED_BLOCK_PRECEDENCE`, `MeasuredObjectiveRecord::branch_precedence`,
  `MeasuredBranchPrecedence::{declared_order, conflicting_blocks,
  unmeasured_order_reason}`, and the `DeclaredPrecedence` / `DeclaredSupport`
  documentation.
* `crates/cs_app/src/objectives.rs`: `measure_block_precedence`,
  `is_objective_block`, `objective_block_number`, `measured_numbers`,
  `RetailBranchConflict`, `RetailObjectiveRow::{completion_effect_sites,
  order_dependency_sites, branch_precedence, conflicts}`,
  `RetailObjectiveCensus::{completion_effect_sites, order_dependency_sites,
  multi_effect_blocks, disjoint_multi_effect_blocks, conflicts,
  conflicting_blocks, needs_unmeasured_order, conflict_combinations,
  declared_order}`.
* `crates/cs_app/tests/accept_f39_e2_block_precedence.rs` (new): 8 tests,
  prefix `accept_f39_e2_`.
* `crates/cs_app/tests/evidence_report_f39_e2.rs` (new): the evidence harness.
* `crates/cs_app/tests/accept_f39_d_objective_branching.rs`: the F39-D fixture
  measurement gains the new required field, and nothing else in it changes.
* Wiring only: none — every edited file is an owner path.

**One observable failure.** Before this stage the question had no production
answer at all. `RetailObjectiveCensus::declares_branching()` answered "does the
original declare branching" (`true`, 1091 sites) and nothing finer: the census
counted *key names per mission* and had no notion of a block, a target, or an
effect's target list. So a caller that needed the only thing that decides whether
a precedence rule is needed at all — *does any block name the same objective
twice?* — was served a total of unrelated sites, and the 1338-block corpus
looked uniform: 1056 completion-effect sites spread over 722 blocks, with no way
to tell the one genuinely ambiguous block from the 269 that declare several
effects on different objectives. The measurement that separates them did not
exist, and inventing the order (rather than measuring it) would have been the
silent shortcut.

## What the original declares, measured

Same corpus as F39-D — 53 mission-scoped reader archives, their `objectives.zrd`
members decoded with the production `.zrd` reader, the numbered `OBJECTIVE<N>`
blocks walked **per block** by `measure_block_precedence`.

| measurement | Value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| mission-scoped readers measured | 53 |
| `OBJECTIVE<N>` blocks declared | 1338 |
| branching declaration sites (F39-D's total) | 1091 |
| … of which **completion-effect** sites | 1056 (`WAKE` 412, `NAP` 417, `KILL` 225, `WAKEUP` 2) |
| … of which **order-dependency** sites | 35 (`TICK_DEPENDS_ON_OBJ`) |
| blocks declaring at least one completion effect | 722 |
| completion-effect sites in all | 1056 |
| sites carrying a number that is not an objective number | 417 (every `NAP` site, and no other) |
| objective numbers the sites name, in all | 1706 |
| widest target list any site carries | 12 |
| blocks declaring two or more **different** effects | 270, in all 24 campaign missions |
| … of those, naming **disjoint** target sets | 269 |
| … of those, naming a **common** objective | **1** |
| sites naming their own block / an undeclared objective | 0 / 0 |

**Two families, not one.** `BRANCH_EFFECT_KEY_VOCABULARY` (the four spellings)
and `BRANCH_ORDER_KEY` (`TICK_DEPENDS_ON_OBJ`) partition F39-D's five-key
branching vocabulary exactly: `completion_effect_sites +
order_dependency_sites == branching_sites` (1056 + 35 = 1091) in every row, and
the retail test asserts it per mission. The split matters because only a
completion effect *acts on* another objective, so only it can collide with
another completion effect; `TICK_DEPENDS_ON_OBJ` names an objective the block is
sequenced behind. Six blocks declare an order dependency and nothing else.

**What a completion-effect site carries.** `WAKE` and `KILL` hold a list of one
to twelve integers; every one of the 417 `NAP` sites holds exactly one integer
followed by exactly one float. **What the float is** — a duration, a weight, a
threshold — is *unmeasured*: nothing in this project has run the original, and
the program behind the record is not decoded. It is carried as
`MeasuredBranchSite::arguments`, an unnamed number, and never as a unit.

**A target is an index into the record's own blocks.** All 1706 measured targets
name a block the *same* mission declares, and none names the block it sits in
(`dangling_sites = 0`, `self_referencing_sites = 0` in every row). So the engine
needs no cross-record naming space for a branch target — that is measured, not
assumed — and the synthetic fixture in
`accept_f39_e2_effects_on_disjoint_objectives_raise_no_ordering_question`
deliberately breaks both halves so `is_closed_over_its_record()` is a reading
rather than a constant.

*(One doc correction in passing: `RetailObjectiveRow::optional_sites` said it
counted `BEGIN_DORMANT` too. It never did — F39-D's rule is
`is_optional_objective_key`, which matches the `INACTIVE<n>` stages and
`INACTIVE_COMPLETION_COUNT` only, so the 1096 `BEGIN_DORMANT` sites stay outside
optionality. The field doc now says what the code counts.)*

**`WAKE` and `WAKEUP` are two spellings in one corpus.** `c1b/m03`
`OBJECTIVE13` carries `WAKEUP [14, 15]` beside `KILL [16]`, while `c1/m02` and
others carry `WAKE`. The two are kept apart, and `BranchEffectKind::from_measured_key`
never maps one to the other: the data does not say they are the same effect.

## The isolated condition: one block, one objective, one unresolved order

The corpus asks "which of two effects wins on one objective?" in exactly one
place:

| | |
| --- | --- |
| mission | `zbd/c3/m05` (campaign mission `m05`, F14-D.1's `m<nn>` leaf rule) |
| container | `ZBD/C3/M05/zrdr.zbd`, SHA-256 `78e7ad3f65747fd8f7f8db77b9b9db4a5d84429203b1d5a6bd2b857210585d17` |
| member | `objectives.zrd`, offset 15718, length 13998, SHA-256 `aed2a5da44086274c4ea3bb1426fd71721a44ee14125d21c82704ab816b164b5` |
| block | `OBJECTIVE8` |
| declaration (as authored) | `WAKE_OBJECTIVE_WHEN_I_COMPLETE [9, 10, 11, 44, 30, 68]`, then `NAP_OBJECTIVE_WHEN_I_COMPLETE [68, 2.0]` |
| shared target | objective **68** |
| record's row | 68 blocks, 49 branching sites (47 effect, 2 order), 31 effect blocks, 47 effect sites, 19 argument sites, 95 targets, widest site 9, 11 multi-effect blocks, 10 of them disjoint, 1 conflict |

The two declarations are named by `MeasuredBranchConflict`
(`block: "OBJECTIVE8"`, `target: 68`, `sites: [Wake{nine…}, Nap{[68], 2.0}]`),
and the mission and block are locatable from `RetailBranchConflict::label()`.

**The inference, stated as one.** That `WAKE`, `NAP` and `KILL` name what happens
to another objective when this one completes is an **inference from the
spelling**, in every doc comment and in this finding; the spellings themselves
are measured. That the integers name objective *numbers* of the same record is
now measured too (closure above). That `NAP`'s float measures a *time* is an
inference from the spelling "nap" and stays one.

**The contrary hypotheses, and what would settle each.**

1. *The first-declared effect wins (the authored field order is the rule).* It is
   the cheapest hypothesis and the corpus **refutes** it as a format fact: every
   effect pair the corpus writes more than once is written **both** ways round —
   `WAKE`/`NAP` 99 vs 81, `WAKE`/`KILL` 93 vs 20, `NAP`/`KILL` 70 vs 34 — and the
   single `WAKEUP`/`KILL` pair is written `WAKEUP` first. A fixed field order in
   the record format would give one direction only. So the order is authored
   per block, and even a format order would carry no engine intent. What would
   settle it: the compiled mission program, or an original run of `m05`.
2. *The last-declared effect wins.* Equally unsupported, and equally
   indistinguishable from (1) from the bytes: the same corpus, the same single
   instance, the opposite reading of the same field. Settled by the same evidence.
3. *`KILL` wins over `NAP`, `NAP` over `WAKE` (a fixed ranking).* The corpus
   cannot rank anything: there is no `KILL`/`NAP` or `KILL`/`WAKE` collision in
   1338 blocks to observe, and `WAKEUP` collides once. So this hypothesis has
   **no instance** in the data — which is a measurement, recorded as
   `conflict_combinations() == [("WAKE+NAP", 1)]`, not a licence to invent an
   order for the pairs that never collide.
4. *Both apply and the objective ends in whichever state the engine prefers.*
   Not separable from (1)–(3) without the program; and it would need a fourth
   state, which no measurement in this project has.
5. *`NAP`'s float changes which effect wins.* Unmeasurable here: the float's unit
   is unmeasured, so no argument about it can be evaluated.

**The verification that was possible, and its result.** The record's bytes are
the whole of what this stage can read, and they do not carry the answer:
`objectives.zrd` is a *declaration record*, not the program that runs — F13-B/C
and F38 own the mission-language instruction table, and the compiled bodies are
still refused at their first counter. No original executable has been run in this
project, and `retail` means file access, not behaviour. So: **the original does
not resolve it, on the evidence available, and this stage records that rather
than choosing.**

## The gate, and what the engine must not do

* `UNMEASURED_BLOCK_PRECEDENCE` is the named verdict, carried by
  `MeasuredObjectiveRecord::branch_precedence` and returned by
  `MeasuredBranchPrecedence::unmeasured_order_reason()` (and `None` when a record
  raises no question, so the verdict cannot be claimed by default), so a refusal
  or a report can name the isolated condition instead of only the absence of a
  rule.
* `MeasuredBranchPrecedence::needs_unmeasured_order()` is the single place the
  "this record needs a rule we do not have" answer is given;
  `RetailObjectiveCensus::needs_unmeasured_order()` is its corpus-wide form.
* The authored order is **recorded and never applied**. `MeasuredBranchConflict::sites`
  keeps the record's order as data; `MeasuredBranchConflict::combination()` is
  **canonical** (sorted by `BranchEffectKind`), so the corpus-wide count does not
  change when a block spells its sites the other way round. That is what
  `accept_f39_e2_the_authored_order_is_measured_not_ranked` pins: reversed input,
  same combination, same counts, reversed `effect_labels()`.
* `DeclaredPrecedence::SyntheticConservative` (`Failure` > `Extraction` >
  `Success`) is **unchanged and still labelled synthetic**. It resolves two
  *terminal outcome requests*; this measurement is about *completion effects on
  other objectives*, which is a different question. Its docs now say so, because
  the task brief conflated the two and a reader of the code should not.
* F39-D's support gate is untouched: attaching the richer measurement still
  leaves an `Installation` record unplayable and still refused by name
  (`ProgramLowerError::UnsupportedProgram`,
  `UNMEASURED_OBJECTIVE_SEMANTICS`). A record whose precedence is
  `Resolved::Unknown` is still refused by name at lowering. Measuring *where*
  the original is ambiguous never makes it *runnable*.

## Test inventory (`accept_f39_e2_*`)

`crates/cs_app/tests/accept_f39_e2_block_precedence.rs` (7 unignored + 1 retail):

| Test | Covers |
| --- | --- |
| `two_effects_naming_one_objective_are_one_measured_conflict` | the isolated condition's shape: one conflict, the shared objective, both sites in authored order, the NAP argument, the canonical combination |
| `effects_on_disjoint_objectives_raise_no_ordering_question` | the 269 measured blocks: disjoint targets raise no question; the order key is not an effect; the closure reading is falsifiable |
| `the_authored_order_is_measured_not_ranked` | the declared order is data: reversed input changes the site order and nothing else |
| `two_sites_of_one_effect_are_not_a_precedence_question` | a count of **different** effects, not of sites: a block spelling one key twice declares one effect and raises no question |
| `conflicting_blocks_are_counted_as_blocks_not_missions_or_conflicts` | three conflicts over two blocks read as two blocks, not three and not one mission |
| `the_measured_effect_vocabulary_is_exactly_four_keys` | the closed vocabulary, the two families partitioning F39-D's, `WAKE` ≠ `WAKEUP`, round-trip, no unmeasured spelling |
| `a_measured_record_carries_the_per_block_reading` | `RetailObjectiveRow::measured()` hands the reading over, the row names its mission's condition, and the support gate still refuses |
| `retail_objective_blocks_declare_one_unordered_completion_effect_pair` (retail) | the whole table above over `$CS_GAME_DIR`, plus the per-row invariants, the F39-D key census against the per-block walk, and the both-directions declared order |

## Measured sensitivity (mutation probes, all observed)

* Conflict detection removed from `measure_block_precedence` →
  `two_effects_naming_one_objective_are_one_measured_conflict` and
  `the_authored_order_is_measured_not_ranked` fail, and the retail test fails on
  the reconciliation (`269` disjoint against `270` multi-effect).
* `MeasuredBranchConflict::combination()` ranked by authored order instead of
  canonical → `the_authored_order_is_measured_not_ranked` fails
  (`WAKE+NAP` against `NAP+WAKE`).
* `is_closed_over_its_record()` stubbed to `true` →
  `effects_on_disjoint_objectives_raise_no_ordering_question` fails.
* The census counting `TICK_DEPENDS_ON_OBJ` as a completion effect → the retail
  test fails on the family reconciliation (`1126` against `1091`).
* `RetailObjectiveRow::measured()` dropping the reading (defaulting it) →
  `a_measured_record_carries_the_per_block_reading` fails.
* `conflicting_blocks()` counting `conflicts.len()` instead of distinct blocks →
  `conflicting_blocks_are_counted_as_blocks_not_missions_or_conflicts` fails
  (`3` against `2`).
* The `multi_effect_blocks` / disjoint / conflict counters keyed on the number of
  **sites** rather than on **different effects** → `two_sites_of_one_effect_are_not_a_precedence_question`
  fails (two multi-effect blocks instead of one).

## One public-API change worth naming

`MeasuredObjectiveRecord`, `DeclaredSupport`, `RetailObjectiveRow` and
`RetailObjectiveCensus` no longer derive `Eq`: F39-E2's reading carries the
measured numbers beside the effect targets, and a float is not `Eq`. Nothing in
the workspace relied on the bound (`cargo clippy --all-targets -D warnings` and
the full test run are green), and the alternative — keeping `Eq` by wrapping the
numbers in a newtype with a hand-written `Eq` — would have bought the derive at
the cost of a subtler equality. `PartialEq` is unchanged.

## Unknown / deferred (not guessed)

1. **Which effect applies to the shared objective, and in which order.** The
   isolated condition is named and located; the answer is unmeasured
   (`UNMEASURED_BLOCK_PRECEDENCE`).
2. **What each effect does.** `WAKE`/`NAP`/`KILL`/`WAKEUP` spellings are
   measured; their meanings stay inferences from the spellings (F39-D unknown #1
   carries the same statement for the key census).
3. **`NAP`'s second number.** 417 sites carry one; a one-off traversal of the
   same production reader counted 42 distinct values, from `0.5` to `170`. Unit,
   domain and meaning unmeasured; carried as `MeasuredBranchSite::arguments`.
4. **`WAKEUP` versus `WAKE`.** Two measured spellings in one corpus, one
   collision-free `WAKEUP` pair; no measurement says whether they are the same
   effect, and the code never merges them.
5. **The declared schema has no completion-effect vocabulary at all.** Nothing in
   `DeclaredObjectiveProgram` can express "completing this wakes that": there is
   no declared form and no runtime counterpart in `cs_sim::objectives`, and this
   stage does not invent one. A later stage may add the declared form *with* its
   runtime support and a refusal for the conflicting shape; that is a different,
   larger slice and is filed as a follow-up rather than smuggled in here.
6. **A block that spells one effect key twice.** Measured: **no** block in the
   installation does (270 blocks declare two or more sites and every one of them
   spells two *different* keys), so `multi_effect_blocks`, the disjoint count and
   the conflicts are the same numbers under either reading. What the original
   would do with a repeated key is unmeasured, and it is not a precedence question
   between two effects: it is recorded as two sites and counted as one effect.
   Follow-up **F39-E6** measures the shape on the archives this census excludes
   and gives it its own named verdict; this stage does not decide it.
7. **The census's denominator** (F39-D unknown #5) is unchanged: mission-scoped
   archives only, so the shared and world-group readers may declare completions
   for the same objectives outside this census.
8. **The terminal-outcome precedence** (`INSTANTWIN` / `INSTANTLOSS`, 24 sites)
   is F39-D's unknown and is not touched here.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e2_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e2 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E2/acceptance.json \
  --artifact-root private/evidence/F39-E2 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the F39-D
findings (`docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`,
unknown #3), the F13-B/C findings (program location and the empty opcode ledger),
the F14-D.1 reader-directory findings (the `m<nn>` leaf rule), the F42-D/t463
findings (the `.zrd` objective-record decoding this census reuses), and the
read-only `$CS_GAME_DIR` listing. No web source was consulted and no original
executable was run.