# M01-B-OBJ-1-FU1: carrying the measured WAKE/NAP/KILL/SLEEP semantics in the cs_sim objective runtime

Date: 2026-10-12. Task: #1362 `M01-B-OBJ-1-FU1` (follow-up to #1358
`M01-B-OBJ-1`, parent #1355 `M01-B-OBJ`). Test prefix
`accept_m01_b_obj_1_fu1_`. Capabilities used: none beyond a code change — every
measurement cited here was already taken under #1358 (`retail` read of
`$CS_GAME_DIR` and read-only static analysis of `CS_ENGINE_IMAGE`) and is
recorded, with addresses, in
`docs/findings/2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`.
**No original executable was run**; nothing here is `verified_original`.

## The problem this task settles

`#1358` measured what the engine image does with an objective's completion
effects. Its closing note flagged that `cs_sim::objectives::runtime` modelled
the effects with a *designed* reading that the code refutes in all three of its
mappings:

| effect | the runtime said | the engine image shows (#1358) |
| --- | --- | --- |
| `WAKE` | moves the target to `Active` | sets state 1, guarded: a target that is not *present* or is *completed* is skipped; a target already in state 1 resets its elapsed time and **ends the whole list** |
| `NAP` | moves the target to `Optional` ("never gates mission success") | sets state 2 (a timed pause), inactive, elapsed 0, and **clears the target's completed flag**; the target re-wakes like a one-element `WAKE` once the declared nap time has elapsed |
| `KILL` | moves the target to `Failed` | clears *present* and *active* on the target and leaves its state alone — a **removal**, not an outcome |
| `SLEEP` | (no runtime kind) | sets state 3, inactive, elapsed 0 |
| order | the authored field order of the record | the fixed order `WAKE` → `NAP` → `KILL` → `SLEEP`, whatever order the block spells |

This note records how the runtime's `ObjectiveState`/transition table and the
`cs_app` lowering now carry those semantics, what of the old designed reading
survives, and what stays unknown.

## The decision, item by item

### 1. Two states join the table: `Napping` and `Asleep`

The engine's four states (0 dormant, 1 awake, 2 napping, 3 asleep) and the
sheet's player-facing states are one axis in the engine and two vocabularies
here. The runtime keeps its sheet vocabulary and adds the two states the
measured effects need, named after the effects that set them exactly as
`cs_content::objectives_engine::EngineState` names them:

* `ObjectiveState::Napping` — engine state 2. A shown objective that a `NAP`
  put aside; it is not being pursued while in this state.
* `ObjectiveState::Asleep` — engine state 3. An objective a `SLEEP` effect
  set. (The engine also sets state 3 when an objective *completes*; the
  runtime's completion still lands in the sheet's outcome states
  `Succeeded`/`Failed`/`Superseded`, which is the pre-existing designed
  vocabulary and is unchanged.)

`Dormant` is **not** added: it belongs to the `BEGIN_DORMANT` family whose
timers stay unmeasured as open questions 3 of the parent note, and a state the
runtime could enter but never leave by a measured rule would be an invention.

The transition rows added are the measured writes, gated by the table's
existing philosophy:

* into `Napping`: from `Pending`, `Active`, `Optional` (any live shown target a
  `NAP` names) and from `Asleep` (the engine's `NAP` write has no state guard);
* into `Asleep`: from `Pending`, `Active`, `Optional`, `Napping` (the `SLEEP`
  write has no state guard);
* out of `Napping` and `Asleep`: `Active` (a wake), the three terminal states
  (a declared completion still lands — F39-D's order-independence property is
  extended to the new states, pinned by
  `accept_m01_b_obj_1_fu1_napping_and_asleep_keep_every_outcome_reachable`),
  and `Asleep -> Napping` (a later nap).

`Hidden` still leaves only through its reveal rule, final states still absorb,
and no same-state row exists, so a nap on an already-napping target is refused
and reported like every other same-state declared move. (The engine would
rewrite state 2 and reset the elapsed time; the runtime's "a declared move to
the state already held is refused and reported" rule is kept because nothing
measured needs the reset — the runtime has no elapsed timer to reset.)

### 2. `moves_to` corrected; `KILL` is a removal, not a move

`CompletionEffectKind::moves_to` now returns `Option<ObjectiveState>`:

* `Wake`/`Wakeup` → `Some(Active)`;
* `Nap` → `Some(Napping)`;
* `Sleep` (new kind, see item 4) → `Some(Asleep)`;
* `Kill` → `None`: it moves no state at all.

A kill instead clears the new per-objective **presence** flag
(`TrackedObjective::present`, queryable as
`ObjectiveRuntime::is_present`) and emits the new event
`ObjectiveEventKind::ObjectiveRemoved { objective, from }` carrying the state
the target kept. Nothing about a kill reads as an outcome: it requests no
terminal result and sets no `Failed`. A killed objective is skipped by every
later `WAKE` (the runtime's measured-guard application, item 3). The engine's
other skip sites — nap *expiry* and the per-objective timers — belong to
families the runtime does not model (the nap timer's unit is unmeasured, item
5; the dormant/awake timers are unmeasured), so presence gates what the
runtime can gate and nothing more. A *declared* move (a condition reaction or
a timer action) naming a killed objective is **not** gated by presence: the
engine has no such path at all, so there is no measured rule to carry, and
refusing one would invent a guard.

The non-cascading guarantee survives untouched: no effect kind moves a target
to `Succeeded`, so an applied effect can never complete an objective and the
queue cannot grow while it drains.

### 3. `WAKE` applies with its measured guards

The phase-7 drain now applies a completion's wake list with the engine's own
rules (0x469af0), per target in list order:

* target not **present** → applies nothing and reports
  `ObjectiveChangeRefused { from: <state kept>, to: Active }` (the engine
  skips silently; the runtime's contract reports every refused reference);
* target in a terminal state (the runtime's "completed") → applies nothing
  and reports `ObjectiveChangeRefused` (the transition table refuses any move
  out of a final state — the same observable the old code produced);
* target hidden and unrevealed → refused and reported, exactly as before (F39
  non-negotiable behavior 5; the engine has no hidden state);
* target already `Active` → the wake's measured early-out: the runtime emits
  the new event `ObjectiveEventKind::WakeStopped { source, objective }` and
  **stops the rest of that completion's wake list**. The engine also resets
  the target's elapsed time here; the runtime has no per-objective elapsed
  timer (the dormant/awake timers are unmeasured), so the stop is the whole
  of the carryable behaviour.
* otherwise → `Active`, through the same `change_objective` path every
  declared move takes.

The stop is tracked per *completing objective*, so two completions in one tick
cannot stop each other's lists.

### 4. The fixed order `WAKE` → `NAP` → `KILL` → `SLEEP` replaces the authored order

`CompletionEffectKind` gains the measured `Sleep` kind (the engine reads
`SLEEP_OBJECTIVE_WHEN_I_COMPLETE`; no retail record spells it, so no lowered
program carries it today — the declared schema in `cs_content` has no `Sleep`
spelling either) and a `engine_order()` rank: `Wake`/`Wakeup` 0, `Nap` 1,
`Kill` 2, `Sleep` 3. When a completion queues its effects they are sorted by
that rank (a stable sort, so two targets of one kind keep their authored list
order), and the drain applies them in that order regardless of the order the
declaration spelled. This is the measured rule that resolves the parent
note's open `zbd/c3/m05` `OBJECTIVE8` question: objective 68 is woken first
and then napped, so it **ends napping**. `Wakeup` takes the wake rank as a
designed choice: the engine image has no reader for the
`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` spelling at all (parent note, open
question 4), so nothing measured orders it; it is kept apart from `Wake` as
the declared schema still carries the spelling.

With the within-completion order now measured, the runtime's
`AmbiguousCompletionEffect` refusal is narrowed to what stays unmeasured:
**two different completing objectives declaring different effects on one
target**. The engine completes one objective per pass, so that collision
cannot arise there; in this runtime two completions may share a tick, and
which of them should apply last depends on the pass-origin rule, which stays
unknown (parent note, open question 2). One declaration carrying two effects
on one target — the shape `zbd/c3/m05` `OBJECTIVE8` spells — is now *accepted
and ordered* by the runtime. (`cs_content`'s declared schema still refuses
that shape at `DeclaredObjectiveProgram::try_new`; relaxing it is content-side
work, filed as a follow-up task, not changed here.)

### 5. The nap's number is the nap time — carried, still never converted

#1358 measured what the number beside a `NAP` target is: the loader's own
diagnostic is "NAP_OBJECTIVE_WHEN_I_COMPLETE has no nap_time specified", a
missing value stores `0.3`, and the engine writes the number into the
target's re-wake timer slot, comparing it against accumulated per-pass steps.
`UnmeasuredNumber` is therefore renamed `NapTime`, with that provenance in
its doc.

What the runtime still does **not** do is interpret it. The unit of the
per-pass step's correspondence to this runtime's integer ticks is unmeasured
(parent note, open question 5: reading the step as seconds is an inference),
so converting the nap time into ticks — and with it *nap expiry*, the rule
that ends a nap by itself — would be an invention. The runtime instead:

* stores the declared `NapTime` on the target
  (`ObjectiveRuntime::objective_nap_time`), exactly as the engine stores the
  slot;
* leaves the nap's end to a **declared** move: another completion's `WAKE`
  (measured: a wake sets an inactive target active whatever it was doing) or
  an explicit objective request. The engine's own expiry path is "woken like
  a one-element `WAKE`", so a declared wake is the same write without the
  unmeasured clock.

The observable consequence is deliberate and tested: two programs whose only
difference is the nap number (0.5 vs 170, the measured corpus ends) still
produce identical event streams, because nothing schedules, compares or
weighs anything on the number.

### 6. `TICK_DEPENDS_ON_OBJ`: mapped, not reimplemented

The measured gate — an objective with a dependency is evaluated only while
the named objective is in engine state 1 — lives on the content side
(`cs_content::objectives_engine::EngineObjectives::is_ticked`, shipped by
#1358). The runtime carries the vocabulary mapping the gate needs: engine
"awake" (state 1) is `ObjectiveState::Active` here, so a future lowering of a
tick dependency asks `objective_state(dependency) == Some(Active)`. No new
runtime machinery is added for it in this task.

### 7. What stays unknown or unmodelled, unchanged

Carried verbatim from the parent note, because this task adds no measurement:
the dormant/awake timer slots and their transitions (open question 3), the
pass-origin rule (open question 2), the nap-time unit (open question 5), the
wake's dormancy bookkeeping (open question 6), whether
`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` is truly inert (open question 4 — an
inference from the exact-match lookup), and whether the image's behaviour
matches this reading when run (open question 7). Additionally, by design and
not by measurement: the runtime's terminal states stay absorbing even though
the engine's `NAP` clears its completed flag — the runtime's completion is an
outcome latch, and letting an effect pull an objective out of a final state
would break both the terminal precedence contract and the effect queue's
non-cascading guarantee.

## What changed, and where

* `crates/cs_sim/src/objectives/state.rs` — the two states, the rows, the
  order-independence extension.
* `crates/cs_sim/src/objectives/runtime.rs` — `moves_to`, the `Sleep` kind,
  `engine_order()`, the presence flag and `is_present`, the phase-7
  application (guards, stop, order), the narrowed ambiguity refusal, the two
  new event kinds, the `NapTime` rename, and the task-prefixed tests.
* `crates/cs_sim/src/objectives/mod.rs` — module docs.
* `crates/cs_app/src/objectives.rs` — the lowering (`lower_effect_kind` and
  `lower_effect` docs and the `NapTime` crossing), the session's dispatch
  arms for the two new events, task-prefixed lowering tests.
* `crates/cs_app/src/accessibility/objective_page.rs` — the two new states
  map to `ObjectiveStatus::Pending` (shown, not being pursued) so the
  accessibility page keeps its exhaustive mapping. No authored program can
  reach the new states through the declared schema today, so the mapping is
  unreachable until a content-side recovery carries them; it is a designed
  placeholder, not a measured UI rule.
* `crates/cs_app/tests/accept_f39_e5_completion_effects.rs` — assertions that
  pinned the refuted mappings (`Nap -> Optional`, `Kill -> Failed`, authored
  effect order, the within-declaration ambiguity) now pin the measured ones.

Evidence class: none of this is `verified_original`; it is a designed runtime
aligned to a static reading of the engine image, and the retail gate tests
that keep original records unplayable are untouched.
