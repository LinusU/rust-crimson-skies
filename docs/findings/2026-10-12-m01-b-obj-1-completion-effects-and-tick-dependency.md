# M01-B-OBJ-1: what `WAKE`, `NAP`, `KILL` and `TICK_DEPENDS_ON_OBJ` do

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-12. Task: #1358 `M01-B-OBJ-1` (parent #1355 `M01-B-OBJ`). Test prefix
`accept_m01_b_obj_1_`. Capabilities used: `retail` (read of `$CS_GAME_DIR`) and
`CS_ENGINE_IMAGE` (read-only static analysis of `crimson.decrypted.exe`, 2,580,480
bytes). **No original executable was run**; nothing here is `verified_original`.
Everything below is a reading of the executable's code, not an observation of
behavior, and says so where it matters.

This note supersedes F39-E2's "unknown #1/#2" for the four keys named in the title
(`2026-10-03-f39-e2-block-completion-effect-precedence.md`): F39-E2 could only read
`objectives.zrd`, which declares the keys; the code that consumes them is in the
engine image. No engine code, bytes or disassembly are reproduced here; addresses
are virtual addresses in the image (PE sections `.text` 0x401000, `.rdata`
0x603000, `.data` 0x619000) so a reviewer can find each claim.

## Where the keys are read

The mission loader in `mission.cpp` (the strings embed `D:\zipper\Crimson\mission.cpp`)
reads `OBJECTIVE1`, `OBJECTIVE2`, … by formatting `OBJECTIVE%d` (0x6260c0, uses at
0x46790b and 0x46900d) and stops at the first number that is absent; the count
is stored in the mission record. **Objective number N is index N−1.** Keys are looked
up with one exact, case-sensitive string comparison that returns the **first**
match in document order (0x57a090 → 0x579ff0).

| Key | Read at | Stored as |
| --- | --- | --- |
| `WAKE_OBJECTIVE` or `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (aliases; the bare one is tried first) | 0x468c0f | list of (number − 1), `-1` terminated |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE` | 0x468c5b | same |
| `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` | 0x468c97 | same (no retail record spells it) |
| `NAP_OBJECTIVE_WHEN_I_COMPLETE` | 0x468cd3 | target (number − 1) and a float |
| `WAKE_OBJECTIVE_WHEN_I_SLEEP` | 0x468dc5 | same list shape (no retail record spells it) |
| `TICK_DEPENDS_ON_OBJ` | 0x4679e1 | one value, number − 1, default −1 (none) |

Measured consequences of the reader:

* **The float of `NAP` is the nap time.** The loader's own diagnostic is
  "NAP_OBJECTIVE_WHEN_I_COMPLETE has no nap_time specified." (0x626964); when the
  second value is missing it stores `0x3e99999a` (0.3 as a float). An integer second
  value is converted to a float; a string or a nested list is the loader's
  "Expecting real value" error path.
* **A record integer `0` ends a list.** `0 − 1` is the `-1` terminator, so
  `[3, 0, 5]` reads as `[3]`.
* **The lists have no bounds check.** `WAKE` and `KILL` each have 15 slots before the
  next field of the engine's record, one of which is the terminator, so 14 targets
  fit; a longer list overruns into the neighbour field. The corpus maximum is 12
  (F39-E2). `ObjectiveRecovery` refuses a longer list rather than model the overrun.
* **`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` is never read.** The image has no such string
  and no bare `WAKEUP` string; the `WAKEUP_…` spellings in the image
  (`WAKEUP_ENEMIES`, `WAKEUP_TURRETS`, `WAKEUP_GENERATOR`, `WAKEUP_SOUND_GROUP`, …)
  belong to the directive table, not to the objective block. The two `WAKEUP` sites
  in the corpus (`c1b/m03` `OBJECTIVE13`) therefore have no reader in the loader.
  That the engine *ignores* them is an inference from the exact-match lookup; the
  recovery keeps them **unrecovered**, named, rather than recording an "ignored".
* **Only the first occurrence of a key counts**, and the lookup descends into
  nested lists, so a key repeated or nested inside a value can shadow. F39-E6 found
  no repeated key in the corpus; recovery treats a repeat as unrecovered.

## Completion: one handler, one order

All code that changes an objective's state (the three callers of the state setter
0x46b160, the wake routine 0x469af0 and the kill routine 0x46b130) lives in one
per-pass function around 0x46a590–0x46afb0. When an objective's completion
conditions are met (0x46a947), the handler:

1. sets the objective **completed**, **inactive**, state **3** (0x46a955–0x46a966);
2. applies `WAKE` (0x46aa35, through 0x469af0);
3. runs the other completion work (sounds, counters, `TIMER_ADJUST`);
4. applies `NAP` (0x46ad73);
5. applies `KILL` (0x46addd);
6. applies `SLEEP` (0x46ae11).

**Precedence is fixed by the engine, not by the record**: `WAKE` → `NAP` → `KILL` →
`SLEEP`, whatever order the block spells them in. This answers the one isolated
condition F39-E2 could not (`zbd/c3/m05` `OBJECTIVE8`, `WAKE [9, 10, 11, 44, 30, 68]`
then `NAP [68, 2.0]`): objective 68 is woken first and then napped, so it **ends
napping**. F39-E2's hypothesis (1)/(2) (authored order) and (3) (a ranking that put
`KILL` first) are refuted by the code; hypothesis (3)'s `NAP` over `WAKE` is
confirmed by order of application. No `KILL`/`NAP` or `KILL`/`WAKE` collision exists
in the corpus; the engine order would make `KILL` the last word on presence (below).

Also measured: **only one objective completes per pass** — the handler records the
first completion and skips the rest of that pass (0x46a94c) — so two objectives
whose conditions are met in the same pass complete in successive passes. The rule
that picks the scan's starting objective (a rotating origin kept at 0x71c128) is
**not** decoded here and stays unknown.

### What each effect does

* **`WAKE`** (0x469af0). For each target in order (up to 15 slots, stopping at the
  terminator): skip a target that is not *present* or is *completed*. If the target
  is already in state 1, reset its elapsed time and **stop the whole list** (later
  targets are not woken). Otherwise set it active, state 1, elapsed 0. A target that
  was in state 0 (dormant) additionally runs the dormancy bookkeeping (its pending
  float at +0x5e0 and the objective named by `HIDE_OBJ`, which is marked completed
  with no outcome); that bookkeeping belongs to the `BEGIN_DORMANT` and `HIDE_OBJ`
  families and is not modelled.
* **`NAP`** (0x46ad73). If the target index is in `0 ≤ i < count`: state 2,
  inactive, elapsed 0 (0x46b160), the target's nap time is set to the declared
  float and the target's **completed flag is cleared**. A napping objective with a
  non-negative nap time accumulates the pass step (the float at 0x9ad744) and, once
  the total reaches the nap time, is woken like a one-element `WAKE` (0x46a702).
  *Unit:* the step is the engine's global frame step, written as 0.125 (0x463f0f),
  0.01 (0x4922a1) and computed values elsewhere; reading it as seconds is an
  inference and stays one.
* **`KILL`** (0x46b130). For each in-range target (stopping at the terminator or an
  out-of-range index): *present* and *active* cleared. The state is left alone. A
  killed objective is skipped by every later wake, nap expiry and timer.
* **`SLEEP`** (0x46b160, state 3). Inactive, state 3, elapsed 0.

State names follow the effects that set them (0 dormant, 1 awake, 2 napping,
3 asleep); the engine stores only the numbers.

### What a `KILL` is not

The runtime in `cs_sim::objectives::runtime` models `Wake` as "Active", `Nap` as
"Optional — never gates mission success" and `Kill` as "Failed". The code above
contradicts all three: a nap is a **timed pause that ends in a wake**, a kill is a
**removal** (it is not an outcome, and it does not set a failure), and a wake is a
state change guarded by flags. That crate is outside this task's owner paths; the
mismatch is reported as a follow-up task.

## `TICK_DEPENDS_ON_OBJ`

Read at 0x4679e1 as one number − 1. Used in both loops of the per-pass function
(0x46a5e0 for the timers, 0x46a83c for the completion test): an objective with a
dependency is **skipped entirely unless the named objective is in state 1**. In
particular a dependency that has *completed* (state 3) or is napping or dormant
stops its dependents from being evaluated at all. The retail corpus uses it 35 times
(M01: 3).

## What is recovered, what is not

`ObjectiveRecovery::read` now recovers a `WAKE`, `NAP`, `KILL`, `SLEEP` or
`TICK_DEPENDS_ON_OBJ` field when the loader would read it: the measured shape, the
first occurrence in its block, and every target among the objectives the loader
loads. They are kept as `cs_content::objectives_engine::EngineDeclaration` per block
and behave as `EngineObjectives` (pure functions: `wake`, `complete`,
`advance_naps`, `is_ticked`, `evaluates_completion`).

For M01 (`zbd/c1c/m01`, 58 blocks, 358 fields): **59 fields recovered — 56
completion-effect fields and 3 tick-dependency fields** — and 299 fields remain
unrecovered in the other families. Across the 53 mission readers: 412 `WAKE`,
417 `NAP`, 225 `KILL` and 35 `TICK_DEPENDS_ON_OBJ` sites are recovered; the two
`WAKEUP` sites are not.

`ObjectiveRecovery::program()` **still refuses**, now naming the other families.

## Unknown, with the evidence that would settle each

1. **How an objective becomes complete** — the conditions (`INACTIVE<n>`,
   `INACTIVE_COMPLETION_COUNT`, `INSTANTWIN`, counters) are other families; the
   function that tests them (0x469a60, 0x469ab0, 0x4697a0, 0x46b090, 0x465910,
   0x465b40) is not decoded here.
2. **Which objective starts a pass** (the rotating origin at 0x71c128) and so which of
   two simultaneously complete objectives completes first.
3. **Dormant and awake timers** (the floats at +0x5d0, +0x5d4, +0x5dc; the
   `WAKE_OBJECTIVE_WHEN_I_SLEEP` list). No retail record spells the last.
4. **Whether `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` is inert.** Inferred from the exact-match
   lookup; an original run of `c1b/m03` would settle it.
5. **The unit of the nap time** (the step is a global frame step; seconds is an
   inference).
6. **The wake's dormancy bookkeeping** (`HIDE_OBJ`, the float at +0x5e0).
7. **Whether the engine's behavior matches this reading when run** — static reading
   only; no original run exists.
