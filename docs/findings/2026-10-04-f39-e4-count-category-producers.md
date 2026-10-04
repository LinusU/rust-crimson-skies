# F39-E4: the counter categories the original declares, and the two it does not

Date: 2026-10-04. Task: F39-E4 "Give CountKind::Disabled and CountKind::Escaped a
measured producer" (Rally #598; the sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has no
`### F39-E4` section, so the task-test prefix this stage uses is
`accept_f39_e4_`, stated here as the sheet states a prefix per stage). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(read-only `$CS_GAME_DIR`). Evidence report: `private/evidence/F39-E4/acceptance.json`,
committed as `docs/findings/evidence/F39-E4.json`.

Question inherited from F39-A/B/C/D (their unknown #7): the engine's five
counter categories and the damage lifecycle's five transitions do not line up.
`LifecycleKind` has `Destroyed`, `PilotBailout`, `OwnershipCaptured`,
`Despawned` and `MissionRemoved`; `CountKind` declares `Destroyed`, `Disabled`,
`Captured`, `Escaped` and `Despawned`. So **two of the five categories could be
counted only by a caller that reported them**, which is why F39-D's category test
had to use a capture to show that a captured convoy does not satisfy a
`Destroyed` condition.

**The short answer:** the original declares neither, and what it *does* spell
where a category could be spelled is a localized label, not a counted
transition. So no producer was added — the five-category vocabulary is **design**,
is labelled design in both vocabularies, and an original record is now **refused
by name** when it counts a category the original does not declare or nothing
measured reports.

## Files and the one observable failure (listed before editing)

* `crates/cs_sim/src/objectives/counters.rs`: the module docs, `CountKind::ALL`,
  `CountKind::label`, `CountKind::needs_declared_reporter`.
* `crates/cs_content/src/objectives.rs`: `is_objective_inactive_stage` (split out
  of `is_optional_objective_key`), `DeclaredCountKind::{all, label, support,
  declared_by_original, name_stem, names_spelling}`, `CountCategorySupport`,
  `MeasuredCategoryEvidence`, `MeasuredCountConditions`,
  `MeasuredTargetKinds`, `measured_category_evidence`,
  `UNDECLARED_COUNT_CATEGORY`, `UNMEASURED_COUNT_CATEGORY`,
  `original_count_category_refusal`,
  `ObjectivesSchemaError::UnmeasuredCountCategory` and its `Display` arm, the
  `try_new` refusal, and the module docs.
* `crates/cs_app/src/objectives.rs`: the F39-E4 section, `measure_count_conditions`,
  `measure_target_kinds`, `RetailObjectiveRow::{count_conditions, target_kinds,
  category_evidence, category_name_counts}`, the census's
  `{target_records, labelled_targets, missions_without_targets, stage_sites,
  threshold_sites, thresholded_blocks, category_names, category_evidence,
  category_missions}` and the `targets.zrd` step of the walk.
* `crates/cs_app/tests/accept_f39_e4_count_category_producers.rs` (new): 7 tests,
  prefix `accept_f39_e4_`.
* `crates/cs_app/tests/evidence_report_f39_e4.rs` (new): the evidence harness.
* `crates/cs_app/tests/accept_f39_e2_block_precedence.rs`: its row fixture gains
  the two new fields, stated empty and `None`.
* Wiring only: none — every edited file is an owner path.

**One observable failure.** `CountKind::from_lifecycle` had no case that could
produce `Disabled` or `Escaped`, and nothing else in the engine reported them, so
a caller had to name the category itself to count it: the count was reachable and
the producer was fiction. Worse, nothing said so at the boundary. A declared
record carrying `Origin::Installation` could declare a `Disabled` or an `Escaped`
count condition and the schema accepted it, so the five-category vocabulary read
as though the original supported it while no subsystem could ever satisfy it. The
new refusal is observable: such a record is now rejected at declaration, naming
the condition, the category and the measured fact behind the refusal.

## What was measured, over the owner's installation

Two pure walks over the objective records, both run by the census and by the
acceptance suite's synthetic fixtures:

* [`measure_count_conditions`] reads each `OBJECTIVE<N>` block's **counted
  conditions** — the `INACTIVE<n>` stages beside their
  `INACTIVE_COMPLETION_COUNT` threshold. This is the only counter the original's
  records actually write.
* [`measure_target_kinds`] reads each `targets.zrd` record's **localized kind**
  (`help_label`) and category (`category_label`) — the surface that reads as
  "what must happen to this actor".

| measurement | Value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| mission-scoped readers measured | 53 |
| …declaring a `targets.zrd` member | 52 |
| …declaring **none** | 1 (`zbd/c1c/m01`) |
| `OBJECTIVE<N>` blocks declared (F39-D's count, unchanged) | 1338 |
| counted-condition stages (`INACTIVE<n>` sites) | 1335 |
| completion-count thresholds (`INACTIVE_COMPLETION_COUNT`) | 130 |
| …F39-D's optionality sites, which the two above partition | 1465 |
| blocks with **both** a threshold and a stage | 129 |
| stage shapes, by names per site | 1 → 35, 2 → 356, 3 → 944 |
| distinct names the counted conditions write | 226 |
| target records declared | 327 |
| …carrying an objective kind (`help_label`) | 289 |
| distinct names over both surfaces | 247 |

The one mission reader with no `targets.zrd` is a **measured absence**, not an
empty reading: `RetailObjectiveRow::target_kinds` is `None` there and
`missions_without_targets()` names it, so "this mission declares no objective
kind" is never reported about a member nobody read. The two surfaces therefore
have different denominators (53 and 52) and neither was widened to the other.

The stage and threshold numbers are also a **reconciliation** of two
independent walks over the same population, not one walk counted twice: F39-D's
flat key census counts every `is_optional_objective_key` site (1465 over the
installation), and this stage's per-block walk splits exactly that population
into stages and thresholds (1335 + 130 = 1465, mission by mission as well). The
retail test asserts the partition, so a walk that drifted from F39-D's census
would fail rather than publish two truths.

### The five categories, measured

| category | declaring sites | what the corpus writes | missions |
| --- | --- | --- | --- |
| `Destroyed` | 107 | `targets.zrd` kind `MSG_OBJ_DESTROY` | 25 |
| `Disabled` | 13 | `MSG_OBJ_DISABLE` (5), `MSG_OBJ_DISABLEENG` (8) | 10 |
| `Captured` | 0 | — | 0 |
| `Escaped` | 0 | — | 0 |
| `Despawned` | 0 | — | 0 |

The shared contract asks for **six** distinctions, not five:
`docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") says conditions
distinguish *disabled, dead, captured, escaped, detached and despawned*, and
**`Detached` has no counterpart in either vocabulary** — not a
`CountKind`, not a `DeclaredCountKind`, not a measured spelling, and not
recorded as an unknown by F39-A/B/C/D either. This stage did not measure it
(what detaching an actor means is F20-C's socket semantics and F36's
docking/transfer rules), so the vocabulary stays five where the contract says
six. Recorded as unknown below and filed as its own task rather than guessed
here.

The classification is one declared rule, `DeclaredCountKind::names_spelling`: the
name is split on `_` and a category claims it when one of its segments begins
with that category's **name stem** (`DESTROY`, `DISABL`, `CAPTUR`, `ESCAP`,
`DESPAWN`), case-insensitively. Segment-based rather than whole-string, because
the measured labels carry a `MSG_OBJ_` prefix, and prefix-based within a segment
because the corpus spells two of the disable labels as one prefix. The acceptance
suite pins that the ten measured part-state spellings (`healthy`, `panels`,
`healthy_part`, …) match nothing, so a stem cannot swallow a state.

The ten missions carrying a disable declaration are `c1/ia1`, `c1b/ia1`,
`c1b/m03`, `c1c/ia1`, `c2/ia1`, `c2b/ia1`, `c3/ia1`, `c4/ia1`, `c5/ia1` (one
each) and `c5/m03` (four: `cargozep1`, `cargozep2`, `cargozep3`,
`sprucegoose`).

### What the counted conditions actually name

The counted conditions name an **actor, a part and a part state**: the largest
spellings are `healthy` (983), `panels` (194) and `healthy_part` (56), with
object names such as `piratezep` (381) and `cargozep2` (142) and part names such
as `reng11` (46) beside them. `zbd/c5/m03` is the clearest instance: 140 stages
and 11 thresholds, `healthy` on 112 of them, and four targets labelled
`MSG_OBJ_DISABLE` — a cargo zeppelin and a Spruce Goose whose engines the mission
writes a per-stage condition for. Its `COMPLETED_SOUND_GROUP` spellings
(`snd_MN3Cargo1Half`, `snd_MN3Cargo1Disabled`) are the corpus's own spelling of
the same idea.

**That a state spelling means anything at all is an inference from the spelling,
and stays one.** Nothing in this project has run the original; the compiled
program behind the record is undecoded (F13-B/C, F38 own the instruction table).
What the measurement supports is bounded and it is what the gate uses: no name
either surface writes spells a captured, escaped or despawned category, and a
localized objective kind is not a counted lifecycle transition.

## The verdict, and the gate it produced

The task offered two outcomes — find the measured transition, or record with
evidence that the original has no such category. The measured answer is the
second, with one refinement worth stating plainly: the original **does** name
disabling, twice, in 13 records across ten missions. It names it as a
localized label on a target and in a sound group, which is *evidence that the
concept exists in the game* and *not evidence of a transition an objective
counter observes*. Turning that label into a producer is exactly the "synthetic
producer" the task forbade.

So:

* `CountKind::Disabled` and `CountKind::Escaped` are **reachable only from a
  caller that reports them**, and that is now stated in queryable form:
  `CountKind::needs_declared_reporter()` is `true` for exactly those two, and
  the declared half says the same thing through
  `DeclaredCountKind::support()`.
* `DeclaredCountKind::declared_by_original()` records what the corpus **spells**
  (destroy and disable), which is a weaker claim than a producer and is
  documented as such. The retail test re-measures all five categories and fails
  if this answer drifts from the corpus, in both directions.
* `original_count_category_refusal` is the single place the gate is decided, from
  two separately measured facts and two named verdicts: the original's records
  must spell the category (`UNDECLARED_COUNT_CATEGORY`) **and** something must be
  able to report it (`UNMEASURED_COUNT_CATEGORY`). On this installation exactly
  one category clears both — `Destroyed` — so an original record may count
  destroyed actors and nothing else.
* `ObjectivesSchemaError::UnmeasuredCountCategory` refuses such a declaration at
  `try_new`, naming the condition, the category and which measured fact refuses
  it. A **newly authored** record may still use all five: it is design, and its
  origin says so.
* F39-D's support gate is untouched. Attaching the richer measurement still
  leaves an `Installation` record unplayable, refused by name with
  `UNMEASURED_OBJECTIVE_SEMANTICS`. Measuring *what the records spell* never makes
  them runnable.

## The contrary hypotheses, and what would settle each

1. *The original's own disabled count is spelled somewhere this census cannot
   see.* Possible and not refuted: the census covers **mission-scoped archives
   only**, so the shared reader (`ZBD/zrdr.zbd`, 220 members) and the world-group
   readers are outside it (F39-D unknown #5; task #597 owns the wider
   denominator). What would settle it: that wider census, or the compiled program.
2. *A counted condition's part-state spelling is the disabled count.* The corpus
   makes this readable — `healthy`, `panels`, `healthy_part` are part states of
   engines and gasbags, and `c5/m03` pairs them with disable-labelled targets and
   `snd_MN3Cargo1Disabled` — but reading a name as a rule is precisely the
   inference F39-D refused for `WAKE`/`NAP`/`KILL` and that F39-E1 refused for the
   dormant lifecycle. Settled by the same evidence as theirs: the mission
   program, or an original run.
3. *Escaping the theatre is declared as a failure instead of a category.* No
   measured spelling supports it: the corpus contains no segment beginning
   `ESCAP` on either surface, and its outcome vocabulary is F39-D's `INSTANTWIN`
   (15) and `INSTANTLOSS` (9). What a mission's failure looks like when a bomber
   leaves is **unmeasured** and belongs to F43's outcome rules.
4. *Ownership capture is a real transition the counter should count.* Plausible as
   a game concept — the corpus has multiplayer flag-capture labels in
   `strings.dll` — and **unmeasured** as a mission-declared category: no record
   spells it. F34/F36 own capture mechanics; if a mission ever counts it, the
   measured declaration will name it and this gate will open by measurement, not
   by edit.

## Test inventory (`accept_f39_e4_*`)

`crates/cs_app/tests/accept_f39_e4_count_category_producers.rs` (6 unignored +
1 retail):

| Test | Covers |
| --- | --- |
| `counted_conditions_name_part_states_not_categories` | the original's only counter: stages beside a threshold, the three measured shapes, and that no name on that surface names any of the five categories |
| `a_target_label_is_not_a_counter_category` | the label surface: both disable spellings are counted, and the category's support stays unmeasured |
| `the_category_vocabulary_is_a_declared_stem_match` | the one classification rule, stem distinctness, case-insensitivity, the ten part-state spellings matching nothing, and declared support against the lifecycle that produces it |
| `an_original_record_may_not_count_an_undeclared_category` | the gate: four categories refused by name with the right verdict each, `Destroyed` accepted, all five accepted for an authored record, and the gate decided in one place |
| `the_unproduced_categories_are_named_where_a_session_reads` | `needs_declared_reporter()` for exactly two categories, labels shared with the declared half, and a bailout/mission-removal counting toward neither |
| `a_measured_record_carries_the_category_reading` | both surfaces merged into one evidence, the three undeclared categories reading empty, and the support gate still refusing |
| `retail_objective_records_declare_no_disabled_or_escaped_count` (retail) | the whole table above over `$CS_GAME_DIR`, the differing denominators, the stages+thresholds partition reconciled against F39-D's independent optionality census, `declared_by_original()` against the corpus in both directions, and the per-mission disable sites adding up |

## Measured sensitivity (mutation probes, all observed)

* The `try_new` refusal removed →
  `an_original_record_may_not_count_an_undeclared_category` fails.
* `names_spelling` made to return `None` → the vocabulary, label, record and
  retail tests fail (4 of 7).
* `needs_declared_reporter` made to return `false` →
  `the_unproduced_categories_are_named_where_a_session_reads` and
  `counted_conditions_name_part_states_not_categories` fail.
* `measure_count_conditions` made to record only a stage's **last** name →
  `counted_conditions_name_part_states_not_categories` fails (the negative
  reading depends on every name being counted).
* The missing `targets.zrd` turned back into a defaulted empty reading → the
  retail test fails on `missions_without_targets()` (`[]` against
  `["zbd/c1c/m01"]`).

## Public-API changes worth naming

`RetailObjectiveRow` gains `count_conditions` and `target_kinds`, so its struct
literal needs the two fields (F39-E2's fixture was updated, with the values
stated rather than defaulted). `ObjectivesSchemaError` gains one variant.
`DeclaredObjectiveProgram::try_new` can now fail on a condition a caller passed
before; `cs_content::objectives::objectives_schema_tests` covers the existing
refusals unchanged, and the new one is covered by the acceptance suite.
`UNDECLARED_COUNT_CATEGORY`'s **text** changed during review (it now states the
bound the measurement was taken under instead of claiming the original has no
such category); a caller matching on that string is matching a diagnostic, not a
stable API, and the verdict it names is unchanged.

## Unknown / deferred (not guessed)

1. **What any counted condition means.** The shapes, names and thresholds are
   measured; the rule that turns a part state into a completed objective is not.
   F39-E1 carries that question for the dormant lifecycle, and this stage did not
   widen it.
2. **A disabled airframe in the original's own records.** `zbd/c5/m03` is the
   strongest candidate corpus-wide (112 `healthy` stages on a cargo zeppelin's
   engine and turret nodes, four `MSG_OBJ_DISABLE` targets,
   `snd_MN3Cargo1Disabled`), and it is still a spelling. A stage that recovers the
   part-state semantics could open this gate **by measurement**.
3. **The wider denominator** (F39-D unknown #5): the shared and world-group
   readers. One of them (`ZBD/C1C/zrdr.zbd`) carries a sixth `MSG_OBJ_DISABLE`
   record, outside this census; task #597 measures that corpus properly.
4. **Escaping the theatre** is not declared as a category anywhere measured. If
   the original does have such a rule it lives in the program, which is undecoded
   (F13-C/F38), or in a reader this census excludes.
5. **Capture as a mission category** stays `CountCategorySupport::Lifecycle`
   (the engine's `OwnershipCaptured` reports it) but `Undeclared` for an original
   record. F34/F36 own the mechanic; nothing measured says a mission counts it.
6. **`count_conditions` and `target_kinds` are not carried in
   `MeasuredObjectiveRecord`.** The content record is unchanged so F39-D's and
   F39-E2's committed measurements stay exactly as published; the F39-E4 reading
   travels on the census row, which is where the walk produces it. A future
   importer that attaches a record to a program and needs the category evidence
   will have to widen that type — a separate, larger change.
7. **The census reads no opcode and no compiled program**, so nothing here is
   evidence of how the original behaves, only of what its files declare.
8. **`Detached` is in the shared contract's six and in neither vocabulary.**
   `docs/contracts/SCRIPT-MISSION.md` requires conditions to distinguish
   disabled, dead, captured, escaped, detached and despawned; the engine's
   `CountKind` and the declared `DeclaredCountKind` both have five, and the
   missing one is `Detached` — no variant, no producer, no measured spelling and
   no prior finding recording it as unknown. It is **not** resolved here by
   inventing a category: what detaching an actor means belongs to F20-C (part
   sockets and detach velocity) and F36 (docking, boarding, transfers), and the
   gate this stage added does not claim anything about a detached actor. Filed
   as its own task so the contract's sixth distinction is tracked against a
   measurement instead of against this stage's silence.

## Independent review

Implementer and reviewer are the same agent **name** (`bunny-alpha-2`) in two
separate sessions; the review session started from the Rally task description
alone, so its context was fresh but it is not a different agent instance, and
under AGENTS.md it is therefore **not independent evidence**. Nothing in this
stage was re-verified against an original run, so no level above `checked` is
claimed anywhere.

**The rebase over F39-E1 changed how this stage reads the second member.**
F39-E1 (merged while this task was in review) split the census walk into
`locate_mission_objective_records` — one definition of "every mission", carrying
each mission's decoded `objectives.zrd` — and made `survey_retail_dormant_reveal`
share it. This stage's second surface is the `targets.zrd` member, so it now
carries those **bytes** on `MissionObjectiveRecord` and decodes them in
`survey_retail_objective_records`, which is the only caller that reads them: a
target record that fails to decode fails the objectives census and cannot fail
F39-E1's dormant walk, and the measured absence (`targets_bytes: None`) is still
carried rather than defaulted. No measured number in this document changed: the
census re-measured on the rebased tree differs from the pre-rebase one only in
its `candidate_tree` and in the three `original_record_refusal` strings this
review reworded — every count, spelling, shape and mission list is identical.

What the review session did independently:

* re-ran the four checks on the branch head and the `accept_f39_e4_` selection
  with `--include-ignored` against `$CS_GAME_DIR` (7/7, retail one included);
* re-derived the measured table from the committed census artifact
  (`stage_sites` 1335, `threshold_sites` 130, `thresholded_blocks` 129, 53
  readers, 52 target readers, 327 records, 289 labelled, 107 destroy over 25
  missions, 13 disable over 10 missions, 0/0/0 for the rest, stage shapes
  1 → 35 / 2 → 356 / 3 → 944, 226 distinct counted-condition spellings) and
  checked every number quoted in this document against it, including the
  part-state and part-name spellings (`healthy` 983, `panels` 194, `piratezep`
  381, `reng11` 46);
* confirmed by reading production code that the three measured producers really
  exist — `DamageResolver::record_lifecycle` for `Destroyed`, `Captured` and
  `Despawned`; the `record_lifecycle(player, LifecycleKind::OwnershipCaptured)`
  call in `cs_sim/src/allies.rs`; and the `CountKind::Disabled` teardown
  exception in `cs_app/src/objectives.rs` — so no claim in this document rests
  on an enum variant alone;
* repeated three mutation probes from scratch: removing the `try_new` refusal
  (1 failure), `declared_by_original()` always `true` (2 failures, one of them
  the retail test), and recording only a stage's last name (1 failure).

What the review session fixed: the harness prose that reported the 52 target
readers as the missions declaring *no* `targets.zrd` (it named 52 for the count
that declares one and 1 for the count that declares none), three copy-paste
references to `evidence_report_f39_e2.rs`/`accept_f39_e2_` inside the E4 harness,
the missing `detached` unknown above, the new reconciliation of the stages and
thresholds against F39-D's independent optionality census, and the wording of
`UNDECLARED_COUNT_CATEGORY` — it read "the original declares no such actor
count", which is a universal claim about a census that deliberately covers only
the mission-scoped readers, so it now states the bound it was measured under and
names the wider denominator it does not cover. The refusal's *verdict* is
unchanged: same four categories refused, same gate, same single place deciding
it.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e4_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e4 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E4/acceptance.json \
  --artifact-root private/evidence/F39-E4 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the F39-A/B/C/D
findings (`docs/findings/2026-10-03-f39-b-continuous-triggers-counters-and-timer-actions.md`,
`.../2026-10-03-f39-c-mission-program-ui-and-dialogue-wiring.md`,
`.../2026-10-03-f39-d-branching-optional-and-failure-validation.md`, unknown #7)
and the F39-E2 finding
(`.../2026-10-03-f39-e2-block-completion-effect-precedence.md`), the F13-B/C
findings (the program is located and its instruction table unmeasured), the
F14-D.1 reader-directory findings (the `m<nn>` leaf rule), the F42-D/t463
findings (the `.zrd` decoding this census reuses), F12's string-catalog findings
(a `MSG_*` label resolves through `strings.dll`, never decoded here), and the
read-only `$CS_GAME_DIR` listing. No web source was consulted and no original
executable was run.
