# F39-D: original branching, optional and failure conditions, validated

> **Correction (F39-D-COUNT, 2026-10-05):** the block counts below that come from the flat walk undercount `BEGIN_DORMANT` (1096 → 1118; sentinel 992 → 1014). See `2026-10-05-f39-d-count-bare-directive-walk.md`.

Date: 2026-10-03. Task: F39-D "Validate original branching, optional and failure
conditions" (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
section `### F39-D`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capability used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F39-D/acceptance.json`, committed as
`docs/findings/evidence/F39-D.json`.

Acceptance case this stage owns: **AC04 — complete supported objectives out of
the common order without deadlocking the program**, plus the sheet's requirement
that the stage's tests cover the failure cases.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/objectives/state.rs`: `ObjectiveState::can_become` (one
  row added), `ObjectiveState::is_outcome_reachable` (new, the order-independence
  rule as a queryable fact), and the `accept_f39_d_` unit test.
- `crates/cs_content/src/objectives.rs`: `DeclaredSupport`, `MeasuredObjectiveRecord`,
  the three measured key vocabularies, `UNMEASURED_OBJECTIVE_SEMANTICS`,
  `support_for`, `with_measured_record`, `is_optional_objective_key`,
  `ObjectivesSchemaError::{DeadSelfReveal, DeadWatch, EmptyMeasurement}`,
  `check_watch`, and the widened `check_reveal`/`check_timer` lookups.
- `crates/cs_app/src/objectives.rs`: `ProgramLowerError::UnsupportedProgram` and
  the refusal at the top of `lower_program`; the retail census
  (`survey_retail_objective_records`, `RetailObjectiveCensus`,
  `RetailObjectiveRow`, `ObjectiveCensusError`).
- `crates/cs_app/tests/accept_f39_d_objective_branching.rs` (new): 9 tests,
  prefix `accept_f39_d_`.
- `crates/cs_app/tests/evidence_report_f39_d.rs` (new): the evidence harness.
- Wiring only: none — every edited file is an owner path.

**One observable failure:** with `Pending -> Succeeded` missing from the
transition table, an objective a reveal rule had just shown (`Hidden` to
`Pending`, never `Active`) could not be completed at all. Every completion
request against it was refused forever, and a completion whose `on_complete`
requests the mission outcome could therefore never request it — so the mission
had **no reachable terminal outcome**. That is a deadlock produced by the state
table rather than by the program, and it is exactly what AC04 forbids. It was
found by probing the production runtime, not by reading the table.

## The retail measurement

`survey_retail_objective_records` walks the read-only installation, opens every
**mission-scoped** reader archive (`zbd/<group>/<mission>/zrdr.zbd`, F13-B's own
`mission_scope` rule), locates its `objectives.zrd` member through the F06
two-key dispatch, decodes it with the production `.zrd` reader, and reads the
numbered `OBJECTIVE<N>` blocks through `cs_content::stunts::objective_state_machine`.

| measurement | Value |
| --- | --- |
| installation SHA-256 | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| mission-scoped readers measured | 53 |
| campaign missions among them (`m<nn>` leaf) | 24 |
| `OBJECTIVE<N>` blocks declared | 1338 |
| branching declaration sites | 1091 |
| optionality declaration sites | 1465 |
| outcome declaration sites | 24 (across 16 of the 24 campaign missions) |
| distinct keys inside the blocks | 55 |
| total key occurrences inside the blocks | 5477 |

**What the original declares.** Three families, all present, with these exact
measured spellings and counts:

| family | key | occurrences |
| --- | --- | --- |
| branching | `WAKE_OBJECTIVE_WHEN_I_COMPLETE` | 412 |
| branching | `NAP_OBJECTIVE_WHEN_I_COMPLETE` | 417 |
| branching | `KILL_OBJECTIVE_WHEN_I_COMPLETE` | 225 |
| branching | `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` | 2 |
| branching | `TICK_DEPENDS_ON_OBJ` | 35 |
| optionality | `INACTIVE1` … `INACTIVE18` (per block index, 18 keys) | 1335 |
| optionality | `INACTIVE_COMPLETION_COUNT` | 130 |
| outcome | `INSTANTWIN` | 15 |
| outcome | `INSTANTLOSS` | 9 |

`BRANCH_KEY_VOCABULARY` and `FAILURE_KEY_VOCABULARY` in
`cs_content::objectives` hold the measured spellings, and
`OBJECTIVE_INACTIVE_STAGE_PREFIX`/`OBJECTIVE_INACTIVE_COUNT_KEY` hold the
optionality ones. `is_optional_objective_key` matches a stage as `INACTIVE` +
digits and the count key exactly, so `INACTIVE_COMPLETION_COUNT` cannot be read
as a stage and a spelling this stage never saw cannot be invented from the
prefix.

**The other 46 keys** inside the blocks are also measured and published by
`RetailObjectiveCensus::vocabulary()` — 5477 occurrences in all, of which the
three families above account for 2580. The largest are
`BEGIN_DORMANT` (1096), `COMPLETED_SOUND_GROUP` (585),
`WAKE_OBJECTIVE_WHEN_I_COMPLETE`'s siblings already listed, `WAKEUP_SOUND_GROUP`
(123), `WAKE_ANIM` (99), `REMOVE_OBJECTIVE_TARGET` (153),
`ADD_OBJECTIVE_TARGET` (76), `TRAVELERS` (75), `WAKEUP_ENEMIES` (67),
`STOP_QUEUED_SOUNDS` (49), `SET_AI_NET` (41), `TICK_DEPENDS_ON_OBJ` (35),
`WAKEUP_ZEP_TURRETS` (31), `DANGER_ZONES_COMPLETED` (31),
`REMOVE_OTHER_TARGET` (27), `COMPLETED_STOPPOINT` (22),
`WAKEUP_GENERATOR` (22), `ADD_OTHER_TARGET` (16), `INSTANTWIN` (15),
`WAKEUP_TURRETS` (14), `START_TAXI` (7), `WARP_VEHICLE` (1), `SET_AI_` (1),
`SET_AI_TEAM` (6), `SET_HELP_LABEL` (37), `mobile` (1), `IDENTITY` (112),
`ANIM_STATE` (64), `DEDG` (130), `Change` (1),
`COMPLETED_ZEPCANNONS` (4), `DANGER_ZONES_COMPLETION_COUNT` (6).

**`BEGIN_DORMANT` is measured but deliberately not classified as optionality.**
It occurs in 1096 of the 1338 blocks and `is_optional_objective_key` does not
match it, so it is outside `optional_sites`. It says a block *begins dormant*,
which is a lifecycle fact about when the objective becomes active, not a
statement that the objective is optional; folding the two together would make
the optionality count a guess. It is recorded as its own constant
(`OBJECTIVE_DORMANT_KEY`) and as unknown #4 below.

## The verdict, and the gate it produced

**The original's objective records declare branching, optionality and outcomes.
No rule behind them is recovered.** A census of key names is a vocabulary
measurement: it says which declarations a block carries, never what one does. So:

* `DeclaredSupport` was added to `DeclaredObjectiveProgram`, derived from the
  record's `origin`. A `SyntheticFixture`/`Designed` record is
  `DeclaredSupport::Authored` and playable; an `Installation` record is
  `DeclaredSupport::Original` and **not** playable, whatever it carries.
* `with_measured_record` attaches what F39-D measured
  (`MeasuredObjectiveRecord`: archive, member, digest, byte length, blocks and
  the three site counts) so a refusal can name numbers instead of only naming
  an absence. It never makes the record playable.
* `lower_program` refuses an unplayable record **first**, by name, with the
  record's own reason (`UNMEASURED_OBJECTIVE_SEMANTICS`). This is the content of
  AC04's word *supported*: there are no supported objectives to complete for an
  original mission today, and a designed progression must not be run in their
  place.

That is `docs/contracts/SCRIPT-MISSION.md`'s rule applied to the measured
state: the program *is* available and its *record* does decode, but the rules
are not recovered, so the mission stays Unsupported for the semantics F39 names.

## The regressions this stage repaired

1. **`Pending -> Succeeded` was an illegal transition** (F39 AC04). An objective
   a reveal rule had just shown is `Pending`, never `Active`, so it could not be
   completed and its `on_complete` could never request the mission's ending. The
   row is added and `is_outcome_reachable` states the resulting rule: from every
   state a declaration can reach, `Succeeded`, `Failed` and `Superseded` are all
   reachable and `Active` is reachable-or-held. The unit test enumerates the
   rows instead of looping over `can_become`, so deleting the row fails it.
2. **A reveal rule could wait for the objective it reveals**
   (`ObjectivesSchemaError::DeadSelfReveal`). An objective leaves `Hidden` only
   through its own reveal rule, and a state change against a still-hidden
   objective is refused, so such a rule has no order in which it fires: the
   mission never shows that objective at all.
3. **A watch could name a state the watched objective already holds**
   (`ObjectivesSchemaError::DeadWatch`, for both a reveal rule and a timer
   start). The runtime fires a watch on a *state change* and no state is
   reachable from itself — from `Active` the only moves are the three final
   states, and the reveal out of `Hidden` reports a reveal rather than a state
   change. A deadline armed by such a watch never arms, which is a program
   waiting on an event nothing can produce.

F39-C left (2) and (3) as "a deferred liveness question, not a dead-declaration
one". They are dead declarations and are now refused at declaration, by name,
with the rule stated.

**Not widened into a refusal.** Two objectives that watch each other's `Active`
are *live*: each can reach `Active`. They stay legal, and
`accept_f39_d_a_mutually_watching_branch_still_fires` runs the session and shows
the second objective being revealed when the first is activated, so the dead-
branch check cannot be silently broadened into "no watch is ever allowed".

## Test inventory (`accept_f39_d_*`)

`crates/cs_app/tests/accept_f39_d_objective_branching.rs` (9 unignored + 1
retail) and `crates/cs_sim/src/objectives/state.rs` (1 unit):

| Test | Covers |
| --- | --- |
| `supported_objectives_complete_out_of_the_common_order` | AC04 minimum scenario |
| `a_revealed_objective_can_complete_before_it_is_pursued` | the regression alone |
| `optional_failure_and_success_stay_distinct` | non-negotiable 5 |
| `a_captured_actor_never_satisfies_a_destroyed_condition` | non-negotiable 2 |
| `a_branch_that_can_never_fire_is_refused_by_name` | the two dead declarations |
| `a_watch_on_an_objective_born_finished_is_refused` | the third dead watch, and the birth states that stay legal |
| `a_mutually_watching_branch_still_fires` | the refusal is not over-broad |
| `an_original_record_is_never_played_as_design` | the support gate |
| `a_measured_record_is_kept_and_a_bare_one_is_refused` | measurement data and its refusal |
| `retail_objective_records_declare_branching_outcomes_and_optionality` | the census over `$CS_GAME_DIR` (retail) |
| `the_state_machine_is_order_independent` (cs_sim unit) | the table's rule |

## Measured sensitivity (mutation probes, all observed)

* `Pending -> Succeeded` deleted → `the_state_machine_is_order_independent`,
  `supported_objectives_complete_out_of_the_common_order` and
  `a_revealed_objective_can_complete_before_it_is_pursued` fail.
* The dead-branch refusals removed → `a_branch_that_can_never_fire_is_refused_by_name`
  fails (the record with the self-reveal now validates).
* The `lower_program` support gate removed →
  `an_original_record_is_never_played_as_design` and
  `a_measured_record_is_kept_and_a_bare_one_is_refused` fail.
* The census's family counting replaced by a constant →
  `retail_objective_records_declare_branching_outcomes_and_optionality` fails on
  the vocabulary/site-total reconciliation.
* The `born_terminal` half of `check_watch` removed →
  `a_watch_on_an_objective_born_finished_is_refused` fails (the reviewer probe).

## Review (bunny-alpha-2, fresh context, same agent instance as the implementer)

Reviewed against this sheet section, `docs/contracts/SCRIPT-MISSION.md` and
AGENTS.md, and the four checks were re-run. The measurement reproduced
independently: the census over `$CS_GAME_DIR` reports install
`b4e780ab84cf31d8…`, 53 readers, 1338 blocks, 55 keys, 5477 occurrences,
1091 branching / 1465 optionality / 24 outcome sites, 24 campaign missions,
16 with an outcome — every figure in the tables above. The `Pending -> Succeeded`
repair is sound on the table's own terms: `Pending` already reached `Failed` and
`Superseded` and only `Succeeded` was missing, which is the shape of an oversight
rather than of a rule, and the row is labelled designed everywhere it is
described. `is_outcome_reachable` does enumerate its targets, so a deleted row
fails the unit test. Four problems were found and fixed on the branch:

1. **`DeadWatch` caught only half of what its own rule claims** (F39-D's second
   regression). The rule is "a watch that can never fire is refused at
   declaration", but the implementation only compared the watched state with the
   watched objective's *birth* state. An objective born `Succeeded`, `Failed` or
   `Superseded` has no row leaving it — verified against
   `cs_sim::objectives::state::can_become`, where no row leaves a final state —
   so a reveal rule or a timer start watching it for *any* other state was
   accepted and then never armed. `check_watch` now refuses that case too, and
   `accept_f39_d_a_watch_on_an_objective_born_finished_is_refused` pins both
   halves: all three terminal births are refused, and the same watch is still
   accepted from `Active`, `Optional` and `Pending`, so the refusal cannot drift
   into "no watch is ever allowed". Those two cases exhaust the possibility,
   because every remaining birth state reaches every remaining watchable state.
2. **The evidence report undercounted the task-test selection**
   (`crates/cs_app/tests/evidence_report_f39_d.rs`). `parse_suite` matched the
   prefix against the whole libtest name, and libtest prints an in-module unit
   test under its module path — the log line is
   `test objectives::state::tests::accept_f39_d_the_state_machine_is_order_independent … ok`.
   So the committed report said `discovered: 9, executed: 9` for a selection that
   runs **10** tests, and omitted a passing assertion from the report. That is the
   incomplete test accounting #353 exists to reject. The prefix is now matched on
   the name's last path segment; the regenerated report discovers, executes and
   passes 10 and lists 10 assertions.
3. **`MeasuredObjectiveRecord::container` was documented as a value it never
   holds.** The doc said "the logical key … as `zbd/<group>/<mission>/zrdr.zbd`",
   but the census assigns production discovery's raw relative spelling
   (`ZBD/C1/M02/zrdr.zbd`); the lowercase key is `RelativePath::logical_key()` of
   it. Both field docs now say which is which, and the retail test asserts that
   `logical_key(container) == "<mission>/zrdr.zbd"` for every row, so the two path
   fields cannot drift apart. `RetailObjectiveRow::member` also stopped being
   derived from `unwrap_or_default()` on a value the search had already proved
   present; it is now the locator's own name, or a named refusal.
4. **Two claims in prose that the data does not support, and two dead helpers.**
   `is_optional_objective_key`'s doc said a spelling the stage never saw "cannot
   be invented from the prefix"; the rule admits `INACTIVE` plus *any* stage
   number, so `INACTIVE19` would be counted. That is deliberate (it is the same
   measured family) and the doc now says so, and the retail test pins the
   measured range: nothing outside `INACTIVE1`…`INACTIVE18` and
   `INACTIVE_COMPLETION_COUNT` may match. The evidence harness's review-method
   prose hardcoded 53/1338/1091/1465/24, so a report regenerated on another
   installation would describe this one's numbers; they are now interpolated from
   the census the same run produced. The harness's two `#[allow(dead_code)]`
   helpers (`families`, `missions`) had no caller and no other harness in the
   repository carries that pattern; the census artifact already publishes both,
   so they are gone.

Reviewer probes run on this branch: disabling `born_terminal` in `check_watch`
fails exactly the new test (observed, above); the implementer's three probes were
re-checked against the code as merged. Nothing else was changed, no protected
path was touched, and no measured value was altered.

## Unknown / deferred (not guessed)

1. **What any declaration means.** `WAKE_OBJECTIVE_WHEN_I_COMPLETE` occurs 412
   times and the spelling is measured; that it *wakes* the named objective when
   this one completes is an inference from the name and is labelled as one in
   every doc comment. Nothing in this project has run the original, and reading
   its files is not evidence of how it behaves.
2. **The compiled program behind each record.** `objectives.zrd` is a config
   record, not the mission program: the compiled bodies are located by F13-B and
   refused at their first counter because the mission-language instruction table
   is unmeasured. F13-C/F38 own that; nothing here reads an opcode.
3. **Precedence inside a block.** 417 blocks declare a `NAP` and 225 a `KILL`
   for the same event. What happens when one block declares both, and in which
   order they take effect, is unmeasured.
4. **Reveal timing and the dormant lifecycle.** Whether the original shows an
   objective when it is born dormant is unmeasured. `BEGIN_DORMANT` occurs in
   1096 of the 1338 blocks and `INACTIVE<n>` plus
   `INACTIVE_COMPLETION_COUNT` in 1335 and 130 of them; that a dormant block
   becomes active once its count is met, and that "active" is what the player
   sees, is an **inference** from the spelling and sits beside it. This is the
   rule F39's `RevealRule` would have to recover, and it is the single biggest
   gap behind `DeclaredSupport::Original`.
5. **The census's denominator.** Mission-scoped archives only. The shared reader
   (`ZBD/zrdr.zbd`, 220 members) and the world-group readers are outside it, so a
   mission may inherit objective declarations this census does not see.
   **Superseded 2026-10-04 by F39-E3**
   (`docs/findings/2026-10-04-f39-e3-installation-scope-objective-declarations.md`),
   which read all nine installation-scope archives: 612 declared members, every
   one decoded, **zero** numbered `OBJECTIVE<N>` blocks among them, so this
   census's denominator is *complete* for the objective-block surface and 1338 is
   the whole installation's count. The same measurement found five objective
   **target** records in the `c1c` world-group reader, which does bound the
   F39-E4 target surface below. Whether a mission resolves a scope reader's member
   at all remains reader-archive precedence (F04/F06), unmeasured.
6. **The 8 campaign missions with no outcome site** declare neither
   `INSTANTWIN` nor `INSTANTLOSS` (the other 16 do). Whether their ending is
   declared elsewhere, or is the absence of an outcome key, is unmeasured.
7. **`Disabled` and `Escaped` still have no producer** (unchanged from
   F39-A/B/C): `LifecycleKind` has no such variant, so `CountKind::Disabled` and
   `CountKind::Escaped` are reachable only by a caller that reports them. F39-D's
   category test therefore uses `OwnershipCaptured`, which the producer does
   have, to show that a captured convoy does not satisfy a `Destroyed`
   condition.
8. **An owner-approved `reconstructed` support variant** (the contract's
   permitted handwritten compatibility reconstruction) does not exist. Only
   `Authored` and `Original` do, and `Original` is unplayable, so a future
   reconstruction needs its own variant rather than a widened `Original`.
9. **The declared-vs-spawned `ActorId` contract** stays documented, not
   enforced (task #594, carried from F39-C and still waiting on F39-D's
   measurements; this stage's census did not change its inputs).

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_d_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_d -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-D/acceptance.json \
  --artifact-root private/evidence/F39-D --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`,
the F39-A/B/C findings (`docs/findings/2026-10-03-f39-b-*.md`,
`.../2026-10-03-f39-c-*.md`), the F13-B/C findings (program location and the
empty opcode ledger), the F14-D.1 reader-directory findings (the `m<nn>` leaf
rule), the F42-D/t463 findings (the `.zrd` objective-record decoding this
census reuses), and the read-only `$CS_GAME_DIR` listing. No web source was
consulted and no original executable was run.