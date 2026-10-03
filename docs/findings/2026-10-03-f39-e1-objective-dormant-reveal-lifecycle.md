# F39-E1: the objective block's dormant/reveal lifecycle — what the installation declares, and what it still does not say

Date: 2026-10-03. Task: F39-E1 "Recover the original objective block's
dormant/reveal lifecycle" (#595), the follow-up to F39-D (#160), whose
unknown #4 this stage answers. Sheet:
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
(non-negotiable behavior 5, "Show objectives only when the original reveal
rules allow"). Contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used:
`retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F39-E1/acceptance.json`, committed as
`docs/findings/evidence/F39-E1.json`.

**Not used and not claimed:** any run of the original executable. Nothing here
is evidence of how the game *behaves*; it is evidence of what its files
*declare*. No reveal rule was recovered, so
`cs_content::objectives::DeclaredSupport::Original` stays unplayable for the
dormant/reveal semantics and this stage changes that verdict not at all.

## The short version

F39-D counted the three declarations and stopped there: 1096 of the
installation's 1338 blocks carry `BEGIN_DORMANT`, 1335 carry an `INACTIVE<n>`
stage, 130 carry `INACTIVE_COMPLETION_COUNT`, and **nothing was known about
what any of them does**. This stage isolated **two** controlled conditions from
the files alone and pinned them in production code and tests:

1. **The positive `BEGIN_DORMANT` argument orders blocks in mission time.**
   Two families of blocks whose `WAKEUP_SOUND_GROUP` cues the original's own
   sound library numbers in sequence (`snd_c2-NW-m2_Ilsa_5` … `_9`, `2` → `77`,
   `6` → `156`, `7` → `210`, `8` → `257`, `9` → `300`) are in that cue order by
   argument as well. A *count* of anything would not reproduce the numbering of
   five independently authored radio lines. Exactly one of the 1096 arguments is
   fractional (`13.5`), which a whole-tick, whole-frame or whole-objective-index
   unit cannot produce.
2. **`INACTIVE_COMPLETION_COUNT` is a threshold over that block's own
   conditions.** Thirty-five families of two or more blocks in one mission
   declare an *identical* `INACTIVE<n>` condition set at *different* thresholds;
   the archetype is `ZBD/C2B/M04`, where objectives 7, 8, 9 and 10 all watch the
   same fourteen Gemini-zeppelin engine conditions (`geminizep` × engine node ×
   `healthy`) at counts 1, 4, 7 and 14, and where the count never exceeds the
   condition list anywhere in the corpus.

What remains **unmeasured**, and therefore not implemented:

* the **unit** of the positive argument (seconds is the only plausible reading of
  a 13.5 and of 300; nothing shipped distinguishes seconds from ticks from a
  mission-relative event number);
* what **satisfying** an `INACTIVE<n>` condition means — the declarations name an
  actor, an optional part and an optional attribute (`healthy`, `panels`), never
  a decoded damage, health or destruction rule;
* whether a satisfied condition is **monotone**, and so whether the thresholds
  are even reachable in play;
* whether the player is shown a dormant objective's text before it activates —
  the display identity is declared *independently* of the dormancy (86 blocks
  are both dormant and the block that carries a display role), and the message
  ids behind those roles are **not resolvable to text** from the installation.

The code therefore measures and refuses, and produces no `DeclaredRevealRule`.

## Files (listed before editing)

- `crates/cs_content/src/objectives.rs` (extend, owner path): the reader
  (`measure_dormant_block`, `measure_dormant_declarations`,
  `objective_block_number`, `inactive_stage_number`), the measured types
  (`DormantReading`, `InactiveCondition`, `MeasuredIdentity`,
  `MeasuredDormantBlock`), the refusals (`DormantReadError`), the new measured
  key constants (`OBJECTIVE_IDENTITY_KEY`, `OBJECTIVE_WAKEUP_SOUND_GROUP_KEY`,
  `OBJECTIVE_COMPLETED_SOUND_GROUP_KEY`, `DORMANT_NO_ELAPSED_TIME`,
  `MEASURED_MAX_INACTIVE_STAGE`, `MEASURED_MAX_CONDITION_ARITY`) and the module
  docs.
- `crates/cs_content/tests/accept_f39_e1_dormant_reveal_declarations.rs` (new, 9
  fast tests): the reader over hand-authored records, including every refusal.
- `crates/cs_app/src/objectives.rs` (extend, owner path): the shared walk
  (`locate_mission_objective_records`, extracted from F39-D's census so both
  surveys have one definition of "every mission"), the census
  (`survey_retail_dormant_reveal`, `DormantRevealCensus`, `DormantRevealRow`,
  `DormantCensusError`), the two controlled conditions
  (`condition_ladders`, `cue_ordered_dated_blocks`, `ConditionLadder`,
  `LadderRung`, `CueOrderedFamily`, `CueOrderedEntry`) and the census queries.
- `crates/cs_app/tests/accept_f39_e1_retail_dormant_reveal.rs` (new, 7 retail
  tests, all `#[ignore]`d).
- `crates/cs_app/tests/evidence_report_f39_e1.rs` (new): the evidence harness.
- This file and `docs/findings/evidence/F39-E1.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data in Git,
no binary file.

## The one observable failure, before the change

Given an installation objective record, **no production function anywhere could
read what a block's dormant/reveal declarations say**. F39-D's census published
*counts* of the optionality keys per mission
(`RetailObjectiveRow::optional_sites`) and the key vocabulary, which is enough
to know `BEGIN_DORMANT` occurs 1096 times and not one byte more about any single
occurrence: its argument's shape, its value distribution, whether the same
argument ever appears beside a condition, whether a count ever exceeds its own
condition list, whether two blocks share a condition set. A future importer
lowering the original had to re-walk `zrd_flat_fields` by hand to learn any of
it, which is exactly the "declaration sites, not rules" hole F39-D recorded —
and there was no place in production to put the answer even if someone measured
it.

## What was measured, and how it was isolated

Every number below is re-measured on every run of the retail suite; a stale
constant fails the test instead of passing.

| measurement | Value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| mission-scoped readers measured | 53 (13 of them declare no block: the five `IA1` and eight `MP*` readers) |
| campaign missions among them | 24 |
| `OBJECTIVE<N>` blocks declared | 1338 |
| blocks carrying `BEGIN_DORMANT` | 1096 |
| … the measured `-1` sentinel | 992 |
| … a positive argument | 104 |
| distinct positive arguments | 41 (from 1.0 to 300.0; exactly one fractional, `13.5`) |
| blocks carrying ≥ 1 `INACTIVE<n>` condition | 271 |
| `INACTIVE<n>` declarations read | 1335 |
| blocks carrying `INACTIVE_COMPLETION_COUNT` | 130 |
| blocks carrying `IDENTITY` | 111 (112 declarations; one block declares two) |
| condition arities | 1 → 35, 2 → 356, 3 → 944 |
| condition attributes | `healthy` 750, `panels` 194 (no other spelling) |
| distinct condition subjects | 141 |
| shared-condition families (≥ 2 blocks) | 53, of which 35 declare > 1 threshold |
| dated/cue-ordered families | 2, both in agreement |
| blocks declaring a `WAKEUP_SOUND_GROUP` | 123 (37 of them a dated block) |
| blocks declaring a `COMPLETED_SOUND_GROUP` | 585 |
| condition parts (the second element's spellings) | 88 distinct |

### The five families every block falls into

The corpus partitions exactly once, which is itself the first piece of evidence
that `BEGIN_DORMANT` is not one undifferentiated "inactive" flag:

| family | blocks |
| --- | --- |
| sentinel `-1`, no condition | 836 |
| sentinel `-1`, **with** conditions | 156 |
| positive argument (never with conditions) | 104 |
| conditions, **no** dormant declaration | 115 |
| neither | 127 |

The **disjointness** of "positive argument" and "carries `INACTIVE<n>`" is
measured, not assumed: **zero** of the 104 dated blocks declares a condition, and
156 of the 992 sentinel blocks does. Whatever the positive argument means, it is
not the same mechanism as the condition ladder.

### Controlled condition A — the argument is an elapsed-time quantity

`ZBD/C1/M02` writes six blocks that both declare a positive `BEGIN_DORMANT`
argument and name a `WAKEUP_SOUND_GROUP`. Five of those cues are the same radio
line with a trailing index, and the indices and the arguments agree in order:

| cue | `BEGIN_DORMANT` |
| --- | --- |
| `snd_c2-NW-m2_Ilsa_5` | 77 |
| `snd_c2-NW-m2_Ilsa_6` | 156 |
| `snd_c2-NW-m2_Ilsa_7` | 210 |
| `snd_c2-NW-m2_Ilsa_8` | 257 |
| `snd_c2-NW-m2_Ilsa_9` | 300 |

`ZBD/C2/M02` gives a second, smaller instance: `snd_c3-HW-m1_FilmDirector_2` at
`2` and `…_8` at `20`. Both families agree; there are no families in which they
disagree. `cue_ordered_dated_blocks()` only compares names that share a prefix
and differ in a trailing index, so it never invents an ordering for a name that
carries none (C2/M01's `snd_HW1First`/`snd_HW1Second` are excluded for exactly
that reason).

**Inference.** The argument orders those blocks the way the designers numbered
their radio cues — that is, it is a quantity that increases through the mission.

**Contrary hypotheses, and why they fail on these files.**

* *It is a count.* 38 appears 38 times, 2 appears 38 times, 15 appears 17
  times: the values are not one mission's per-objective sequence, and a count
  has no reason to reproduce the cue ordering.
* *It is an objective index.* The corpus contains 41 distinct values including
  `13.5`, `77`, `181`, `257` — a fractional value and values far above any
  mission's objective numbering (1338 blocks over 53 readers, none above 300)
  cannot both be indices. And `ZBD/C3/M03 OBJECTIVE2` carries `13.5` outright.
* *It is a frame or tick count.* One fractional argument rules out any
  whole-tick unit; `13.5` ticks is also not a number a designer types.
* *It is a mission-relative event ordinal* (e.g. "the 77th thing that happens").
  Not separable from seconds by any shipped file — this is the residual
  unknown, and it is why `DormantReading::ElapsedTime` carries no unit.

**Subsequent verification.** Would need an original run (owner-supplied, task
#358's `REF-OWNER-FIRST-CAPTURE`) or the mission program's compiled body
(F13-C/F38), neither of which any agent has.

### Controlled condition B — the count thresholds that block's own conditions

35 families of two or more blocks in one mission declare an **identical**
`INACTIVE<n>` condition set with **different** `INACTIVE_COMPLETION_COUNT`s. The
archetype, `ZBD/C2B/M04`:

| block | conditions | count | identity |
| --- | --- | --- | --- |
| `OBJECTIVE7` | the 14 `geminizep` engine nodes × `healthy` | 1 | — |
| `OBJECTIVE8` | the same 14 | 4 | — |
| `OBJECTIVE9` | the same 14 | 7 | — |
| `OBJECTIVE10` | the same 14 | 14 | `PRIMARY` 2, `MSG_BRF_HWM4_OBJ2` |

The same shape appears in `ZBD/C1C/M01` (counts 2, 6, 10, 12 over the same 12
conditions), `ZBD/C3/M03` (3, 5, 7, 10 over 12), `ZBD/C4/M04` (1, 2, 3, 4 over
6) and sixteen other families. Across the whole corpus:

* a count is **never** larger than its own condition list — the one exception is
  the one block that has a count and no condition at all
  (`ZBD/C4/M03 OBJECTIVE52`, count 2);
* it **equals** the list in 16 blocks and is **smaller** in 113.

**Inference.** `INACTIVE_COMPLETION_COUNT` is a threshold over the conditions the
same block declares, and the ladder of blocks sharing a set with rising
thresholds is a staged progression over that one set.

**Contrary hypotheses, and why they fail on these files.**

* *It names a stage index* (`INACTIVE_COMPLETION_COUNT: 4` means "activate stage
  4"). The measured counts are frequently smaller than the number of stages
  (14 conditions at count 1, 12 conditions at counts 2/6/10/12), so it cannot be
  an index into a set it exceeds.
* *It counts a mission-global event stream* ("the 4th thing that happens"). Then
  four blocks sharing the same fourteen conditions would have no reason to
  declare thresholds 1, 4, 7, 14 in ascending order; and the corpus's
  correlation between the count and the ladder rung is too tight (35 families)
  for a global counter.
* *It is "how many of the conditions must be *inactive*"* — i.e. the count names
  the conditions that must *stop* being true. This is the same number with the
  opposite polarity, and it cannot be separated from the files (see the next
  section). It is a live ambiguity, not a resolved question.

**Subsequent verification.** An original run of one campaign mission
(C2/M04 is the smallest of the ladder families), with the objective list read
before and after each rung fires.

### What an `INACTIVE<n>` condition says, and what it does not

A condition is a list of one, two or three texts. Read as `(subject, part,
attribute)`:

* **subject** — 141 distinct actor names: zeppelins (`piratezep`,
  `geminizep`, `workersvoyagezep`, `cargozep1`, `vostokzep`, `dantezep`, …),
  ground vehicles (`fuel_truck01`, `tiedown01`), turrets, balloons,
  `pickup_objective`, `player`.
* **part** — 88 spellings: engine and gasbag node names (`reng11`, `leng42`,
  `gasbag1`), weapon nodes (`utur1`, `noseballgun41`), and the same words used
  without a part (`healthy`, `healthy_part`, `healthy_balloon`, `thlthy`,
  `tank`, `box`, `panelleft1`).
* **attribute** — exactly two spellings: `healthy` (750) and `panels` (194).

The same vocabulary appears in the mission archive's own `zeppelins.zrd`
member, which declares each zeppelin's `healthy` list as `[gasbag1, panels]`, …
with a `num_healthy_required` beside it, and its `engines` as
`[leng11, leng12, leng21, …, reng32]`. So the second element of a three-element
condition is a **node of the actor's own airframe record** and the third is a
state word drawn from the same vocabulary.

**Measured.** The conditions name an actor, an optional part of it and an
optional attribute spelling. **Not measured.** What "healthy" is a predicate
over, what makes it stop being true, whether it is monotone once false, and
whether the counted event is the *loss* of the state or its *presence*. The last
one matters: if the counted event were the presence of `healthy`, a block
declaring count 1 over fourteen healthy engines would complete the moment the
mission loaded. That reading makes four of the ladder families in C2B/M04 fire
in the first frame, which no mission author would ship — so **the loss reading is
the consistent one**, but consistency is not an observation and this stage does
not implement it.

### The visibility side: what the player is shown is declared elsewhere

`IDENTITY` carries `(role, ordinal, [message id])`: 81 `PRIMARY`, 29
`SECONDARY`, 2 `TERTIARY` declarations across 111 blocks, 79 of them naming a
`MSG_BRF_*` message. **86 blocks are both dormant and carry a display identity**,
so "dormant" cannot be read as "hidden": the original declares, in two
independent places, when a block may become active and what the objective is
labelled.

What the player actually sees is **not** recoverable from the installation:

* the two shipped generated headers (`ASSETS/SCRIPTS/RESOURCE.H`,
  `RESRC1.H`, read through the production ROF member walk and header reader)
  define **no** `MSG_*` id at all — 0 of their 820 defines;
* the ids' names do exist, as a counted name table in `strings.dll`'s `.data`
  (measured out of band during this session, not by a production reader and
  **not committed**: 5161 NUL-separated `MSG_*`/`FONT_*` spellings including
  every `MSG_BRF_*` an objective names), while the texts live in `strings.dll`'s
  112 `RT_STRING` blocks (1792 rows). 5161 ≠ 1792, so the pairing is not
  positional, and nothing shipped pairs a name with a row id.

So whether the original *shows* a dormant objective's text before the block
activates is a real, named unknown — and it is the half of non-negotiable
behavior 5 that a `RevealRule` would have to recover.

## What the code does with all this

- `measure_dormant_declarations` reads the four measured declaration families
  out of a decoded record and refuses every shape this stage did not measure:
  a `BEGIN_DORMANT` that is not exactly one finite number, a negative argument
  that is not the measured `-1`, a count that is not one integer, a condition
  with no text or with more elements than the measured three, a stage numbering
  that is not `1..=N`, a sound group that is not one non-empty name, and an
  `IDENTITY` of the wrong shape or with an element of the wrong kind. Failing
  loudly is the point: a block that silently vanished would read as a block that
  declares nothing dormant.
- `DormantReading` names the sentinel and "an elapsed-time quantity in an
  unmeasured unit", and nothing more. `InactiveCondition` keeps its arity and
  its spellings verbatim — `healthy` is a measured string, not an enum variant,
  because turning it into one would be a decoded rule this stage does not have.
- `MeasuredDormantBlock::count_exceeds_conditions` and
  `count_without_conditions` are production queries, so a future lowering refuses
  the two degenerate shapes by name instead of defaulting them.
- The census publishes the whole population (per-mission rows, per-block
  measurements), both controlled-condition families and the aggregations above,
  so a later stage can re-derive any of it from the installation rather than
  from this document.
- **No** `DeclaredRevealRule` variant, no `cs_sim` change, no UI change. The
  gate F39-D built stays exactly as it was.

## Test sensitivity

`cargo test --workspace --locked -- accept_f39_e1_ --include-ignored` runs 20
tests: 12 fast (cs_content, CI) and 8 retail (cs_app, `#[ignore]`d, run locally
and by the reviewer with `CS_GAME_DIR` set).

Nine mutations were applied to production source, measured and reverted (the
harness was a scratch script under `private/`, **not** committed; each row below
is a run that was observed, not an estimate):

| mutation | caught by |
| --- | --- |
| `read_dormant` maps any negative argument to the sentinel | `…every_unmeasured_shape_is_refused_by_name` |
| `read_condition` accepts a four-element condition | the same |
| `inactive_stage_number` drops its all-digits rule, so `INACTIVE+1` parses as stage 1 | `…only_inactive_with_digits_is_a_stage` |
| the `1..=N` stage-numbering check dropped | `…stage_numbering_is_either_measured_or_refused` |
| `IDENTITY` keeps only the last declaration instead of all of them | `…an_identity_reads_as_role_ordinal_and_message` |
| `measure_dormant_declarations` matches any `OBJECTIVE*` key, so `OBJECTIVE_DELAY` counts as a block | `…only_objective_with_digits_is_a_numbered_block` (fast, so CI kills it too) and `retail_…every_mission_record_is_measured_whole` |
| `cue_ordered_dated_blocks` compares cue names without requiring a trailing index | `retail_…dated_arguments_order_the_original_s_own_cue_sequence` |
| `condition_ladders` keys families by mission alone, ignoring the condition set | `retail_…shared_condition_sets_carry_several_thresholds` |
| `counts_above_conditions` counts only blocks that have conditions | `retail_…only_the_condition_free_block_declares_an_unreachable_count` |

**9 mutations, 9 killed.** The third row is the one that needed a test change
rather than a code change: with only `INACTIVATED`/`INACTIVE_A` in the fixture
the mutation survived, because a `u32` parse refuses those spellings anyway and
the digits rule looked redundant. It is not — `u32::from_str` accepts a leading
`+`, so `INACTIVE+1` would have read as stage 1. `INACTIVE+1` and `INACTIVE-1`
are now in the fixture and the mutation is killed.

Two production decisions are **not** covered by a mutation and are stated here
so a reviewer can check them by reading:

* `read_dormant`'s treatment of a **zero** argument as an unmeasured refusal
  (not `ElapsedTime(0.0)`) is covered by the same test as the negative case, and
  `read_completion_count`'s refusal of a float count by the same one;
* the census's *refusing* rather than skipping an unmeasurable reader
  (`DormantCensusError::Declaration`) is a behaviour of the code path no
  installation exercises — every block of this installation measures — so it is
  asserted only through the fast reader's own refusals, not through a retail
  fixture.

## Review (bunny-2, fresh session, same agent instance as the implementer)

Reviewed against this sheet, `docs/contracts/SCRIPT-MISSION.md` and AGENTS.md,
and the four checks were re-run on the rebased tree. **This is not independent
evidence**: the reviewer is the same agent instance that implemented the work,
in a session that started with no memory of it. The measurement reproduced on
its own — install `b4e780ab…`, 53 readers, 1338 blocks, 1096 dormant (992
sentinel, 104 dated), 271 staged blocks over 1335 conditions, 130 counts, 111
identity blocks over 112 declarations, 53 condition families of which 35 declare
more than one threshold, and the two cue-ordered families with
`77/156/210/257/300` and `2/20` — every figure in the tables above.

The stage's central judgement is sound and was left alone: nothing was decoded
into a rule, `DeclaredSupport::Original` was not widened, and the two
controlled conditions are labelled as inferences with the contrary hypotheses
that fail on the shipped files *and* the one that does not. Five real problems
were found and fixed on the branch:

1. **A refusal named the wrong declaration.** `read_identity` reported a
   malformed `IDENTITY` element as `NonTextConditionElement { stage: 0 }`,
   whose message reads `OBJECTIVE7: INACTIVE0 element 0 is not text and names
   nothing` — a stage key the block never declared, for a declaration the stage
   does measure. There is a new `DormantReadError::IdentityElement { block,
   index, wanted }`, which names the position and what that position is measured
   to hold, and `IdentityShape` now covers only the arity refusal its doc claims.
   `…an_identity_refusal_names_the_identity` pins all three positions, both
   empty-text positions, and that every refusal carries its own block.
2. **The census recovered the block by parsing an error message.**
   `declared_block_of` split the rendered `Display` output on `": "` to get the
   block name back, so a block whose name ever held that separator would have
   been misreported. `DormantReadError::block()` is now the structured way to
   place a refusal and the string parsing is gone.
3. **A cue declaration of an unmeasured shape was dropped silently.**
   `WAKEUP_SOUND_GROUP` and `COMPLETED_SOUND_GROUP` were read through a helper
   that returned `None` for anything that was not one text element, so a block
   declaring a cue the reader cannot read looked exactly like a block declaring
   none — which is the precise failure the module promises never to allow, and
   it would empty controlled condition A rather than fail it. Both keys are read
   strictly now (`SoundGroupShape` refusal), and the retail suite passing is the
   measurement that **every** one of the installation's 708 cue declarations
   (123 activation, 585 completion) is one non-empty name.
4. **A measured figure was reported as a different kind of figure.** The
   evidence harness's review-method prose interpolated
   `identity_declarations()` into a sentence that read "…and *112 blocks* carry
   an IDENTITY display declaration": 112 is the number of declarations, over 111
   blocks. The template now interpolates both, and also states the measured
   dated-plus-activation-cue population (37) that controlled condition A is
   drawn from.
5. **A test comment contradicted its own assertion**, and three measured claims
   had no test at all. The retail ladder test's doc said "38 of those" where the
   assertion and this finding say 35. `OBJECTIVE_COMPLETED_SOUND_GROUP_KEY` was
   pinned by nothing: a wrong spelling would have read as "no block declares
   one" and passed. Four census queries (`completed_sound_group_blocks`,
   `wakeup_sound_group_blocks`, `dated_wakeup_sound_group_blocks`,
   `condition_parts`) now measure 585 / 123 / 37 / 88, so both cue spellings and
   the 88 part spellings are asserted against the installation rather than
   assumed from the key's name.

Three smaller corrections, in the same spirit — a claim that was asserted but
not measured, a doc that overstated a code path, and coverage that only existed
where CI cannot run it:

* `measure_dormant_declarations`' doc said the blocks come back "in the order
  the original's own numbering follows". That is now **measured**: the retail
  suite asserts `1..=n` in declaration order for all 53 readers, so the claim
  survives instead of resting on nothing.
* `DormantReading`' doc said the *unit* of the positive argument was unmeasured,
  which is true, and left the larger inference implicit: that the argument is a
  *time* at all is also an inference from the key's spelling plus controlled
  condition A, and a mission-relative event ordinal is not excluded by any
  shipped file. The type docs now say so. (This is a wording fix; the variant
  keeps its name, which is about the reading, and the finding keeps the label.)
* The `OBJECTIVE*`-prefix mutation was only killed by the retail suite, so CI
  could not see it. `…only_objective_with_digits_is_a_numbered_block` covers the
  digits rule, nine near-miss spellings and a record that declares
  `OBJECTIVE_DELAY` beside its blocks.

The evidence harness also gained the repository's own JSON self-check
(`assert_well_formed_json`, as `evidence_report_t465.rs` carries it) over **both**
documents it writes: `dormant-reveal-census.json` is only ever hashed and
committed, so a malformed artifact would otherwise be committed as if it were a
measurement. The census artifact now also publishes the three new queries and
their per-value breakdowns.

Checked and left alone: the shared `locate_mission_objective_records` helper
keeps F39-D's census behaviour identical and its `1..=N` block numbering and
member-name handling unchanged; `condition_ladders` and
`cue_ordered_dated_blocks` are queries that report a co-occurrence and never a
rule; the twice-decoded installation (each survey calls the shared walk) is a
simplicity trade, now stated in the helper's doc instead of being described as a
cache; and `unknowns: []` matches the convention every `implemented` report in
this repository follows, with all seven limits named in this finding and in the
report's own `review.method`.

Three reviewer probes were applied to the fixes above, measured and reverted:
reading an unreadable cue as a placeholder name instead of refusing it is killed
by `…an_identity_refusal_names_the_identity`; matching any `OBJECTIVE*` key is
killed by `…only_objective_with_digits_is_a_numbered_block`; and returning a
fixed block from `DormantReadError::block()` is killed by the same refusal test.
No measured value was changed by the review, and no protected path was touched.

`main` moved under this review: F39-E2 landed in the same two source files, so
the branch was rebased onto it and both conflicts resolved by keeping this
stage's shared `locate_mission_objective_records` helper while F39-E2's three
row fields and its `branch_precedence` reading sit on top of it, measured on
`record.document`. F39-D's 10 tests and F39-E2's 8 were re-run on the rebased
tree (all green) so the resolution is checked from both sides, and the committed
report's `candidate_tree` is the tree this review tested.

## Unknown / deferred (not guessed)

1. **The unit of `BEGIN_DORMANT`'s positive argument.** Ordered in mission time
   by controlled condition A; seconds, ticks and a mission-relative event number
   are not distinguished by any shipped file. Affected content: the 104 dated
   blocks of 1338. Resolving task: an original run (owner capture) or F13-C's
   decoded program.
2. **What satisfying an `INACTIVE<n>` condition means**, and whether the counted
   event is the loss or the presence of the named state. Affected content: all
   271 staged blocks and every threshold over them. Same resolvers.
3. **Whether a satisfied condition is monotone.** If it is not, the thresholds
   of a ladder may be reached in an order the data does not imply, and AC04's
   "complete supported objectives out of the common order" cannot be satisfied
   from the record alone.
4. **142 of the 271 staged blocks declare no count at all.** Whether that means
   "all of them" or "any of them" is unmeasured, and it is the largest single
   group of blocks whose completion condition this stage cannot state.
5. **Whether the player sees a dormant objective's text before it activates.**
   Measured: the display identity is independent of the dormancy, and the message
   ids are not resolvable to text (0 `MSG_*` defines in the shipped headers).
6. **`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, `NAP_…` and `KILL_…`** (412, 417 and 225
   declarations) remain F39-D's inference from the spelling. This stage did not
   touch them; they interact with dormancy (a dormant block is presumably what a
   wake acts on) and that interaction is unmeasured.
7. **The compiled mission program behind each record** (F13-C/F38) and the
   shared/world-group readers outside the census denominator (F39-D unknown #5)
   are unchanged.

None of the seven is a failure of this stage's assertions: every row in the
census resolved, and each unknown is a *limit on the claim* rather than an
unresolved measurement. `unknowns` in the evidence report is empty for that
reason, and the report's `claim` is `implemented`, never `verified_original`.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e1_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e1 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E1/acceptance.json \
  --artifact-root private/evidence/F39-E1 --require-pass
```

## Sources

- F39-D's finding and census this stage extends:
  `docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`
  (`RetailObjectiveCensus`, `MeasuredObjectiveRecord`, `DeclaredSupport`).
- `docs/findings/2026-10-03-f39-b-continuous-triggers-counters-and-timer-actions.md`
  for `RevealRule` and the "measure rather than assume" rule this stage follows.
- `docs/findings/2026-10-03-f27-e-1-measured-binding-gate.md` for the shape of a
  stage that measures "not measurable here" instead of inventing a value.
- F12-G's header/string-id correlation for the `MSG_*` negative measurement's
  method: `docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`.
