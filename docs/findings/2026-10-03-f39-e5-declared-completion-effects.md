# F39-E5: a completion-effect vocabulary for the declared schema, and the shape that is refused

Date: 2026-10-03. Task: F39-E5 "Give the declared objective schema a
completion-effect vocabulary with runtime support" (Rally #599). The sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has
no `### F39-E5` section — the `F39-E*` stages come from the owner-resequenced
F39 plan (#356) — so the task-test prefix this stage uses is `accept_f39_e5_`,
stated here as the sheet states a prefix per stage. Shared contract:
`docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"). Capability
used: **ordinary build/test only** — this stage measures nothing new on the
installation and therefore produces no evidence report; the retail numbers it
reasons from are F39-D's and F39-E2's, cited below.

## Files and the one observable failure (listed before editing)

* `crates/cs_sim/src/objectives/runtime.rs`: `UnmeasuredNumber`,
  `CompletionEffectKind`, `CompletionEffect`, `ObjectiveSpec::completion_effects`,
  `RuntimeError::{AmbiguousCompletionEffect, SelfCompletionEffect,
  EffectArgument}`, `ObjectiveRuntime::{add_objective, check_uncontested_targets,
  queued_completion_effects, queue_completion_effects,
  apply_completion_effects}`, `change_objective`, `declared_event_count`, and
  the phase table in the module docs.
* `crates/cs_sim/src/objectives/state.rs`: the `Pending -> Optional` and
  `Active -> Optional` rows, and
  `accept_f39_e5_a_set_aside_objective_can_still_be_completed_or_resumed`.
* `crates/cs_sim/src/objectives/mod.rs`: the stage section in the module docs.
* `crates/cs_content/src/objectives.rs`: `COMPLETION_EFFECT_KEY_VOCABULARY`,
  `DeclaredCompletionEffectKind`, `UnmeasuredQuantity`, `DeclaredCompletionEffect`,
  `DeclaredObjectiveState::is_terminal`, `DeclaredObjective::completion_effects`,
  `ObjectivesSchemaError::{AmbiguousCompletionEffect, SelfCompletionEffect,
  DeadCompletionEffect, UnfiredCompletionEffects, EffectArgumentShape}`,
  `check_completion_effects`, `check_state_target`'s revised rule,
  `declared_synthetic_completion_effects`, and the F39-E5 module-docs section.
* `crates/cs_app/src/objectives.rs`: `lower_effect_kind`, `lower_effect`, the
  `completion_effects` mapping in `lower_program`, the F39-E5 module-docs section.
* `crates/cs_app/tests/accept_f39_e5_completion_effects.rs` (new): 8 tests,
  prefix `accept_f39_e5_`.
* Mechanical only, no behaviour change: `completion_effects: Vec::new()` added to
  the existing `ObjectiveSpec`/`DeclaredObjective` literals in
  `crates/cs_sim/tests/accept_f39_b_objective_runtime.rs` (9 sites) and
  `crates/cs_app/tests/accept_f39_d_objective_branching.rs` (6 sites). Every
  other declaration in those suites is about a different rule, and an empty list
  is the whole of the behaviour before this stage.
* Wiring only: none — every edited file is an owner path.

**One observable failure.** Before this stage, 1056 measured declaration sites
had no typed representation anywhere in the engine. `DeclaredObjective` carried
`on_complete: DeclaredCompletion` (Continue | Requests(outcome)) and nothing
else, and `ObjectiveSpec` had no field that could name another objective, so a
program could not say "completing this wakes that" at all: there was no declared
form, no runtime counterpart and no lowering. Nothing in the workspace could
fail, because nothing could even be written down.

## What is measured, and what this stage decided

The measurements are F39-D's and F39-E2's, not this stage's. They are cited here
so a reader can check the decisions against them:

| fact | source |
| --- | --- |
| `WAKE…WHEN_I_COMPLETE` 412, `NAP…` 417, `KILL…` 225, `WAKEUP…` 2 — 1056 sites in 722 of 1338 blocks | F39-D (`BRANCH_KEY_VOCABULARY`), F39-E2 |
| every target names an objective of the **same record** (1706 targets); none names its own block | F39-E2 |
| every `NAP` site carries exactly one extra number (42 distinct values, 0.5 … 170); no other effect site carries one | F39-E2 |
| exactly **one** block in 1338 declares two different effects for the same objective: `zbd/c3/m05` `OBJECTIVE8`, `WAKE [9,10,11,44,30,68]` then `NAP [68, 2.0]`, shared target 68 | F39-E2 |
| the authored field order is not the rule: every conflicting pair is written **both** ways round (`WAKE`/`NAP` 99 vs 81, `WAKE`/`KILL` 93 vs 20, `NAP`/`KILL` 70 vs 34) | F39-E2 |

Four decisions follow, each with the alternative it rejected:

1. **Targets are `ProgramSymbol`s of the same program.** The closure is measured
   (all 1706 targets name a block of the same record), so no cross-record naming
   space has to exist; a program symbol is the right identity, and a dangling
   target is refused at declaration by name. *Rejected:* a mission-wide id space,
   which the measurement says is unnecessary.
2. **One entry per (effect kind, target), not one per site.** The record writes a
   site with a list of one to twelve targets; the engine acts on one objective at
   a time, and the per-target model is what makes "two different effects name one
   objective" expressible at all. *Rejected:* mirroring the site's list, which
   would make the contended shape a nested-list comparison.
3. **The nap's number is `UnmeasuredQuantity`/`UnmeasuredNumber` — data with no
   unit.** 417 sites carry one and nothing measured what it measures (a duration,
   a weight and a threshold are all consistent with the bytes), so the engine
   carries it verbatim across the lowering boundary and never reads it: the move
   a nap performs is the same whatever the number says, which
   `accept_f39_e5_a_napped_objective_is_set_aside_whatever_the_number_says` pins
   with `0.5` and `170`. *Rejected:* a `Duration`, which would invent the unit the
   measurement refused.
4. **Two different effects naming one objective is refused at declaration, by
   name, in both the schema and the runtime** (`AmbiguousCompletionEffect`). This
   is the one instance F39-E2 could not resolve, and the authored order cannot
   substitute because the corpus writes every conflicting pair both ways round.
   *Rejected:* applying the declaration's field order, i.e. inventing the rule.

**The state each effect moves its target to is designed vocabulary**, published as
`CompletionEffectKind::moves_to`: `Wake` and `Wakeup` → `Active` ("being
pursued"), `Nap` → `Optional` ("never gates mission success"), `Kill` → `Failed`.
`Wake` and `Wakeup` stay two spellings that currently perform the same move,
because the corpus contains both and no measurement says they are the same
effect; splitting them later needs a measurement, not a guess. `Optional` was not
reachable from a live objective before this stage, so the table gained
`Pending -> Optional` and `Active -> Optional` — rows only *add* reachability, so
F39-D's order-independence is untouched and is re-checked from the new states.

## The runtime shape, and why it is a queue

`docs/contracts/SCRIPT-MISSION.md`: *"Actions do not directly recurse into
callbacks. Maintain ordered queues and define when a new event is eligible for
observation."* So completing an objective does not call anything: it *queues* its
effects, and phase 7 of the declared phase order (after counters, conditions,
triggers, signals, objectives and timers, before the outcome is resolved) drains
them. Three properties make the drain safe, and each is testable:

* **No cascade is possible.** No effect kind moves its target to `Succeeded`, so
  an applied effect can never complete an objective and therefore can never queue
  another effect. The drain also takes its due set before applying the first
  effect, so anything queued while draining waits for the next tick.
* **The order is program order and decides nothing.** The queue is filled in the
  order objectives completed (a `BTreeMap` walk) and each objective's effects
  keep their declared order. A contested target cannot be declared, so that order
  never resolves a conflict.
* **An effect is attributed to the objective that declares it.** The
  `ObjectiveChanged` for a target reports under the *completing* objective's
  symbol even when a timer drove the completion, and the completion's own event
  keeps the requesting declaration's key. The session's objective display needed
  no change: an effect is an ordinary declared state change.

Reveal rules still govern visibility, so an effect naming an objective the player
has not been shown is refused *and reported*
(`accept_f39_e5_an_effect_on_a_hidden_objective_is_reported_not_dropped`) rather
than quietly revealing it.

## The gate is untouched

`DeclaredSupport::Original` stays unplayable and `lower_program` still refuses
such a record by name with `UNMEASURED_OBJECTIVE_SEMANTICS`:
`accept_f39_e5_an_original_record_with_effects_is_never_played` builds the *same
declarations* over an installation origin and checks both halves. A declared
vocabulary for what a record says is not a recovery of what it means, and this
stage measured nothing new about the original to change that.

## Test inventory (`accept_f39_e5_*`)

`crates/cs_app/tests/accept_f39_e5_completion_effects.rs` (8) and one unit test in
`crates/cs_sim/src/objectives/state.rs`:

| Test | Covers |
| --- | --- |
| `a_completion_wakes_the_objectives_it_names` | the whole production path: one declared deadline expiry completes one objective and its four effects each move their own target on the same tick, under the completing objective's key, in authored effect order, with the display moved and nothing queued |
| `effects_apply_on_the_completion_tick_and_only_once` | eligibility (same tick, not the next) and the latch: an idle tick moves nothing, and an explicit re-arm of the deadline runs the timer again while the repeated completion is refused by name and no effect replays |
| `a_napped_objective_is_set_aside_whatever_the_number_says` | `0.5` vs `170`: identical streams and identical display, plus the value arriving verbatim on the lowered effect |
| `an_effect_on_a_hidden_objective_is_reported_not_dropped` | reveal rules still govern visibility; the refused move is reported |
| `two_effects_naming_one_objective_are_refused_by_name` | the refusal in the schema in both declaration orders, in the runtime in both registration orders, the message naming both kinds and the shared objective, and the agreeing pair still legal |
| `dead_completion_effects_are_refused_by_name` | self-effect, target born terminal (all three final states), effects declared by an objective born terminal, a nap without its number, a number on a wake, and a dangling target |
| `the_effect_vocabulary_is_the_measured_one` | the four spellings are members of `BRANCH_KEY_VOCABULARY` and round-trip, `WAKE` ≠ `WAKEUP`, an unmeasured spelling names nothing, only a nap carries a number, the four kinds survive the boundary, and none completes its target |
| `an_original_record_with_effects_is_never_played` | the support gate |
| `state.rs`: `a_set_aside_objective_can_still_be_completed_or_resumed` | the two new rows, and that order-independence still holds from every state that can reach `Optional` |

## Measured sensitivity (mutation probes, all observed)

* `apply_completion_effects` not called from `step` → 4 of the 8 tests fail
  (the four that watch a target move).
* The schema's `AmbiguousCompletionEffect` refusal replaced with a no-op →
  `two_effects_naming_one_objective_are_refused_by_name` fails.
* The runtime's `AmbiguousCompletionEffect` refusal replaced with a no-op, and
  the lowering dropping the nap's number (`argument: None`) → 6 of the 8 fail,
  including the refusal test (its runtime half) and the number test.
* The `Pending -> Optional` / `Active -> Optional` rows removed → the nap cannot
  apply and the nap tests fail.

## Unknown / deferred (not guessed)

1. **What each spelling does in the original.** The four spellings, their counts
   and their targets are measured; that `WAKE…WHEN_I_COMPLETE` *wakes* the named
   objective is an inference from the spelling, and the state this stage moves it
   to is designed. Nothing here ran the original executable.
2. **Which effect wins the one contended block** (`zbd/c3/m05` `OBJECTIVE8`,
   objective 68). Unmeasured; refused at declaration rather than ordered. F39-E2's
   `UNMEASURED_BLOCK_PRECEDENCE` carries this from the measurement side.
3. **`NAP`'s number.** 417 sites, 42 distinct values, no unit, no domain, no
   meaning. Carried, never interpreted.
4. **`WAKEUP` versus `WAKE`.** Two measured spellings; the engine currently gives
   them the same designed move because nothing measured a difference. They are
   never merged in the vocabulary, so a measurement can split them.
5. **`TICK_DEPENDS_ON_OBJ`** (35 sites, F39-D's fifth branching key) is an
   ordering declaration, not an effect, and has no declared form here either.
6. **`CountKind::Disabled`/`Escaped` still have no producer** (F39-D unknown 7),
   unchanged: this stage adds no lifecycle producer.
7. **The census's denominator** (F39-D unknown 5) is unchanged; the shared and
   world-group readers may declare completions outside the 1338 measured blocks.
8. **`DeclaredSupport::Original` is still unplayable.** Every original mission
   remains Unsupported for exactly these semantics.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e5_ --include-ignored
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, the F39-D findings
(`docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`,
its measured key table and unknown #3/#4), the F39-E2 findings
(`docs/findings/2026-10-03-f39-e2-block-completion-effect-precedence.md`: the
per-block reading, the isolated condition, the both-directions refutation and its
unknown #5, which this stage closes for the *declared* side only), and the
existing F39-B/C/D suites whose literals this stage extended with one empty field.
No web source was consulted and no original executable was run.
