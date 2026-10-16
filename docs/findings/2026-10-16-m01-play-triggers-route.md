# M01-PLAY-TRIGGERS (#1439): the wake-transition timing, the spelled call, and the headless route

Date: 2026-10-16. Task: `M01-PLAY-TRIGGERS` (Rally #1439), the parent
integration step of `M01-PLAY-TRIGGERS-OBSERVE` (#1480),
`-PARTS` (#1481), `-ANIMS` (#1482) and `-DEPLETION` (#1483). Capabilities
used: `retail` (read-only `$CS_GAME_DIR`) plus build/test. No `gpu`, no
`audio`, **no original run** — nothing here is `verified_original`; a Rally
merge awards `checked` only.

Implementer: `bunny-alpha-2`. The two runtime changes this document cites were
authored by `Devin SWE-2/swe2-max-1` (Rally claim on this task, 2026-10-16)
and finished and evidenced here. The reviewer's identity and whether its
context was fresh are recorded in the `complete_review`/`submit_for_review`
notes.

## 1. What the parent adds

The four slices each made one world fact observable in the live composition
and are merged. The parent contributes the two runtime mechanisms M01's own
win chain needs and that no slice owned, plus the headless route that drives
them:

1. **wake-role directives emit on the block's wake transition**, not on its
   completion (`cs_script::runtime::MissionState::step_with_countdown`);
2. **`CALL_ANIMATION`'s spelled target is consumed at the statement's own
   publication** (`mission_animations::CallAnimationLedger`), so a called
   record is observable on the tick its caller's statement publishes;
3. the acceptance suite
   `crates/cs_app/tests/campaign/m01_play_triggers_route.rs` — one scripted
   headless flight over the composed retail M01 that settles the run on the
   record's own `INSTANTWIN`.

## 2. The wake-transition timing is measured, and here is the citation

The owner note of 2026-10-16 requires the exact measurement for the claim that
a wake-role directive runs on the **wake transition** rather than on
completion, and requires it kept out of any original-behaviour claim if the
role table only classifies directives without stating the timing.

It does state the timing, and the timing is measured in the engine image, not
inferred from the classification:

* The source is `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
  (the same document `cs_content`'s role table cites as `FINDING_B`), section
  *`BEGIN_DORMANT`* and *Objective-to-objective state directives*, where the
  **wake call `0x469af0(objs, idx_array)`** is disassembled. Its measured
  per-record effect list includes, verbatim:

  > `WAKE_ANIM` (`+0x53c`/`+0x540`): `0x523820(name)` → animation, `0x465e30`
  > → target object, `0x4edda0(anim, obj, 0, 0, 0)` — executes the animation.

* The same effect list carries `WAKEUP_ENEMIES`, `WAKEUP_TURRETS`,
  `WAKEUP_ZEP_TURRETS`, `WAKEUP_GENERATOR`, `WAKEUP_SOUND_GROUP`, the `COUNTER`
  `ON_WAKEUP` triple and the dormant-only `RESET_TIMER`/`HIDE_OBJ` — exactly
  the set `cs_content::mission_control::DirectiveOperation::role` classifies as
  `DirectiveRole::WakeEffect`, and the classification is by that measurement.
* The **completion pipeline** (`0x46a94c`, the same finding, *Completion
  pipeline*) lists the completion effects in order — the class sound, the HUD
  post, the `INSTANTWIN`/`INSTANTLOSS` checks, `COUNTER ON_COMPLETE`,
  `WARP_VEHICLE`, the AI-target writes, `START_TAXI`, `SET_HELP_LABEL`,
  `STOP_QUEUED_SOUNDS` and the `+0x1c` wake array. `WAKE_ANIM` is **not** in
  it. The previous runtime emitted the wake-role directives from the
  completion path, which is why M01's `OBJECTIVE18` was circular: its own
  `WAKE_ANIM pzhomebase` armed the landing nodes its `hooked_to_klondike
  EXECUTED` gate reads, and it could only run after the gate closed.

**What the runtime implements, and what it does not claim.** The measured
order (wake, not completion) is reproduced. The *tick boundary* is a designed
reading of it: the original runs the wake effects inside the `0x469af0` call,
whereas `MissionState` emits each wake-role site on the first tick its block's
facts row reads `ObjectiveLifecycle::Awake`, under the same execution key the
completion pass would have used, so the consumed guard suppresses a second
emit. M01's chain reaches its terminals through this reading; a same-call
ordering would need the wake pass to sit inside the completion pipeline rather
than on the next facts fold, which the current script host does not model.

**The initially-awake case.** The set is rebuilt from `facts.objectives` each
tick, so the first tick reads every block the seed loads `Awake` — a block
loaded awake would not have been through `0x469af0` in the original. For
M01's three `WAKE_ANIM` gate blocks (`OBJECTIVE11`/`OBJECTIVE15`/`OBJECTIVE18`)
this does not arise: each is **woken** by the chain, not loaded awake, which
is exactly why the completion-path emission made them circular. Whether any
other M01 block is loaded awake *and* carries a wake-role site is not measured
here and stays an open unknown of the tick-boundary reading, not a claim about
the original.

## 3. The spelled call is measured; its activation is designed

M01's `OBJECTIVE11` gate reads `wv_drop_copilot RUNNING`. The #1482 finding
records that no record's stored `ANIMATION_REFS` table names `wv_drop_copilot`
anywhere in M01's closure, so the prescribed table-driven chain could never
start it — the gate was unreachable.

The **spelled name is measured**. Every `CALL_ANIMATION` (24),
`STOP_ANIMATION` (25) and `INVALIDATE_ANIMATION` (27) event's payload stores
the addressed animation name as a NUL-terminated ASCII string at payload byte
`4`; measured over **3 893** such events in M01's mission and camera carriers,
100 % with no exception (`animation::events::SPELLED_NAME_CLAIM`,
`m01-anim.control-call-payload-name-measured`). `AnimationRecordFacts::call_targets`
exposes the names the record's own statements spell; `wv_initiate_hookup`'s
stream spells `wv_drop_copilot` and `wv_pickup_copilot` among its eight calls,
while its table names only four.

**What starts a spelled call is designed.** `CALL_ANIMATION_ACTIVATION_CLAIM`
(`m01-anim.call-animation-activation-designed`): the named record is started
through the record player on the tick its statement publishes, the same
designed rule the landing path's `animation_refs` chain already states. What
the 2000 engine did with a call — whether the named record started then,
queued, or replaced a running sibling — is unmeasured and stays this claim's
unknown. Only opcode 24 activates; `STOP_ANIMATION`/`INVALIDATE_ANIMATION`
spell the same field and nothing acts on them.

The designed landing chain (`#1498`'s `LANDING_CALL_CHAIN_CLAIM`) was narrowed
to the **remainder**: table names no `CALL_ANIMATION` event of the same record
also spells. On the owner's records that remainder is empty, so the spelled
calls are the whole chain and each fires exactly once.

## 4. The negative leg still never closes its gate

`accept_m01_play_triggers_anims_a_wake_that_never_fires_never_closes_the_gate`
(#1482's member) is unchanged in what it asserts — the landing node is never
armed, a body that would land runs nothing, the gate's animation is never
observed and the gate never completes. Its fixture changed only in how the
"event never fires" leg is expressed: because `WAKE_ANIM` is a measured
wake-role site, an *awake* block runs its wake effects regardless of a
condition that never holds, so the leg now keeps block 0 **dormant**
(`LifecycleDecl::dormant(None)`) — a block the lifecycle model never admits to
`Awake` is a directive that never fires. The positive member and the other
members are untouched. The owner note of 2026-10-16 explicitly requires this
leg to keep never closing its gate, and the reviewer must re-check it.

## 5. What stays designed or unknown

Carried forward, with their resolving measurements:

| # | what | claim | resolving measurement |
| --- | --- | --- | --- |
| 1 | the drop-off beat (the original plays a cutscene) | `m01-dropoff.designed-placeholder` | an owner original capture of the wreck pass |
| 2 | the tick boundary of a wake-role emission (see §2) | `m01-wake-role-emission-tick-designed` | the wake call's call site inside a stepped game loop |
| 3 | a spelled `CALL_ANIMATION` starts its record at the statement's publication (see §3) | `m01-anim.call-animation-activation-designed` | tie a payload name to its statement's timing in the original |
| 4 | `STOP_ANIMATION`/`INVALIDATE_ANIMATION` do nothing | same claim id as 3 | the same measurement |

The #1482 finding's own designed table (the node-arming rule, the landing
detection and window, the node-position join) is unchanged by this task.

## 6. The acceptance suite

`crates/cs_app/tests/campaign/m01_play_triggers_route.rs` adds one member
under the parent prefix, `accept_m01_play_triggers_route_a_scripted_flight_reaches_instantwin`
(`#[ignore = "requires CS_GAME_DIR"]`). It composes the retail M01 stage once,
with the recovered engine lifecycle model, and drives one scripted flight
through the production schedule only:

1. the player's own body is placed through the components the physics step
   commits and the stage-4 observer reads, and flies the leg from its composed
   start to `workersvoyagezep`; `OBJECTIVE3` (block 2) completes on the first
   observed tick inside the record's measured 700 m radius;
2. the eighteen `workersvoyagezep` engine parts leave play through the
   production damage authority (`DamageResolver::resolve`), completing the
   `INACTIVE` ladders `OBJECTIVE6` (5), `OBJECTIVE8` (7) and `OBJECTIVE9` (8);
3. the record's own `WAKE_ANIM` arms `wv_manual_land`; the body flies the low
   pass and lands inside the row's window, running `wv_initiate_hookup` — the
   drop-off/pick-up beat. The placeholder (claim
   `m01-dropoff.designed-placeholder`) waits for the gate block `OBJECTIVE11`
   to read `Awake` before the pass: the chain arms the node (block 48's own
   wake) *before* block 49's 5 s nap wakes the gate block, and the gate reads
   `wv_drop_copilot RUNNING`, a state the landing's call publishes for the one
   tick it runs — the original's cutscene timing is not recovered, so the
   designed placeholder places the pass where the record's own gate can
   observe it;
4. the four group-1 `rusdevastator_*` leave the world, so `OBJECTIVE16` (15)
   completes; group 2 reads its measured `0`, so `OBJECTIVE17` (16) completes;
5. `OBJECTIVE18`'s own wake arms the `piratezep` node; the body lands there,
   running `hooked_to_klondike`, which closes the block's own gate;
6. `OBJECTIVE18` wakes `OBJECTIVE24`, whose `INSTANTWIN`
   `Finish(Succeeded)` settles the run.

The mission clock is compressed (one host step covers 0.5 s) because the win
chain's own `NAP` re-wakes spell 123 s; M01-B-WAKE's drive already records
that nothing in the lowered program reads the tick-to-second ratio. The
member asserts the terminal's outcome, source and requesting block, that every
intervening gate really completed on the live world's own state, that the
observed part rows and group counts really hold the win facts, and that exactly
the two route landings ran.

The **windowed** clause of the parent acceptance ("each trigger fires from the
player's actual position or actions in the windowed mission") is **not** met
here and is not claimed: it stays with `M01-PLAY-TRIGGERS-WINDOWED` (#1486),
which owns the driven windowed run and its `gpu` evidence, as the #1480
finding already records.

## 7. Evidence

Commands and their exit codes are recorded in the `submit_for_review` summary.
The retail member composes the owner's installation read-only and writes no
evidence report of its own: this task's required capability is `retail` (plain
build/test plus read access), so a `private/evidence/<TASK-ID>/acceptance.json`
report is not part of it.
