# M01-LC-DIRECTIVE-B: measured runtime semantics of the objective lifecycle and target directives

Date: 2026-10-06. Task: `M01-LC-DIRECTIVE-B` (#680), "measure objective
lifecycle and target directive semantics". Parent: `M01-LC-DIRECTIVE-MEANING`
(#675). Depends on `M01-LC-DIRECTIVE-A` (#679), whose parser map this document
consumes. Capability used: `retail` (read-only `$CS_GAME_DIR`). Evidence
report: `private/evidence/M01-LC-DIRECTIVE-B/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-DIRECTIVE-B.json`.

**Everything below is static code evidence** from disassembling the owner's
executable. The original program was never run for this task; nothing here is
`verified_original` runtime behavior. Where a semantic could not be recovered
from the code alone it is recorded as **unknown** rather than guessed.

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_ENGINE_IMAGE` |
| Format | PE32 executable, image base `0x400000` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| Address convention | virtual address (VA) |
| Error/assert logger | `0x415330(message, line, file, 0x200)`; `file` = `D:\zipper\Crimson\mission.cpp` |
| Tools | `r2`/`rabin2`/`objdump` on a read-only copy |

## The functions the measurements come from

| Address | Role |
| --- | --- |
| `0x46a490` | `CZMission::Update` — mission-timer expiry, clock advance and early-out |
| `0x46a5ce..0x46a7d6` | Update pass 1 — per-objective lifecycle timers |
| `0x46a7dc..0x46af43` | Update pass 2 — per-objective condition evaluation and completion |
| `0x46a94c` (inline in pass 2) | the ordered completion-effect pipeline |
| `0x46ae98..0x46b07a` | post-pass mission-outcome aggregation and end-of-tick work |
| `0x469af0` | wake an objective from a `−1`-terminated index array (cap 15; returns `−1` on exhaustion, else the truncating index) |
| `0x46b130` | kill one objective by index |
| `0x46b160` | force one objective to a lifecycle state by index |
| `0x469a60` | `INACTIVE` condition evaluator |
| `0x469ab0` | `DANGER_ZONES` condition evaluator |
| `0x4697a0` | `ANIM_STATE` condition evaluator |
| `0x46b090` | `TEST_COMPLETE` counter predicate |
| `0x469dc0` | counter write op (`ON_WAKEUP`/`ON_COMPLETE`) |
| `0x465910`/`0x4658d0`/`0x465850` | `DEDG` evaluator and its group counters |
| `0x465b40`/`0x4659b0`/`0x465a70` | `TRAVELERS` evaluator and its member predicates |
| `0x4693d0`/`0x469440`/`0x469600` | `COUNTER` record builder, sub-triple parser, counter registry |
| `0x465e30` | memoized name→object resolver (zclass-7 lookup `0x4d0280`) |
| `0x466260` | lazy name-chain iterator for target-list records |
| `0x46a1b0`/`0x46a1f0` | `+0x4c` "other target" flag set/clear on a resolved object |
| `0x46a230`/`0x46a270` | `+0x4d` "objective target" flag set/clear |
| `0x46a2b0`/`0x46a2d0` | `START_TAXI` / `SET_HELP_LABEL` consumers |
| `0x46a000` | `SET_AI_NET` consumer |
| `0x469e20` | `SET_AI_TEAM` consumer |
| `0x469f70` | `SET_AI_ATTACK_RADIUS` consumer |
| `0x46a0b0` | `COMPLETED_ZEPCANNONS` consumer |
| `0x46a0d0` | `COMPLETED_STOPPOINT` consumer |
| `0x46c510`/`0x46c550`/`0x46c5a0`/`0x46c5c0` | mission-timer set/adjust/start/stop |
| `0x463c10`/`0x463c20`/`0x463c30` | mission WON / LOST / end-sequence |
| `0x463be0`/`0x463bf0`/`0x463c00` | mission getters: `+0xc58` won, `+0xc5c` lost, `+0xc54` ended |

## The objective record

Objectives live in an array at `this+0xc4c`, count `this+0xc48`, stride
**`0x5e4`** (1508) bytes. At parse time the record is zeroed (`rep stosd`,
377 dwords) and then given these measured defaults: `+0x8 = 1` (alive),
`+0x558 = 1` (HUD notify pending), `+0x10 = −1` (no tick dependency),
`+0xd8 = −1` (no `HIDE_OBJ`), `+0x580 = −1`/`+0x584 = −1` (DEDG unarmed),
`+0x598 = −1` (TRAVELERS group unarmed), `+0x5b4 = 1` (TRAVELERS required
count), `+0x5e0 = 0` (no reset timer). With no `BEGIN_DORMANT` the record is
left **awake**: `+0xc = 1`, `+0x5c8 = 1`, `+0x5d0 = 0`.

| Offset | Measured content |
| --- | --- |
| `+0x0` | identity class: `PRIMARY`→1, `SECONDARY`→2, `TERTIARY`→3 |
| `+0x4` | HUD ordinal (the `IDENTITY` integer) |
| `+0x8` | alive flag — `0` = killed, skips all processing |
| `+0xc` | active flag — `1` = awake, `0` = dormant/napping/done |
| `+0x10` | `TICK_DEPENDS_ON_OBJ` index, stored zero-based (`dec`'d at parse) |
| `+0x14` | latched completed flag |
| `+0x18` | `END_TIMER` marker (stop mission timer on completion) |
| `+0x1c` | `WAKE_OBJECTIVE`/`WAKE_OBJECTIVE_WHEN_I_COMPLETE` `−1`-terminated index array (aliases at one field) |
| `+0x58` | `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` index array |
| `+0x94` | `KILL_OBJECTIVE_WHEN_I_COMPLETE` index array |
| `+0xd0`/`+0xd4` | `NAP_OBJECTIVE_WHEN_I_COMPLETE` target index / nap seconds |
| `+0xd8` | `HIDE_OBJ` objective index |
| `+0xdc` | `WAKE_OBJECTIVE_WHEN_I_SLEEP` index array |
| `+0x118`/`+0x11c`/`+0x120` | `WAKEUP_GENERATOR` name / resolved generator / added count |
| `+0x124`/`+0x128..` | `WAKEUP_ENEMIES` count / name array (cap 10) |
| `+0x150`/`+0x154`/`+0x170..` | `WARP_VEHICLE` destination table / count (parser key unspelled in M01) |
| `+0x270`/`+0x274..` | `WAKEUP_TURRETS` count / name array |
| `+0x29c`/`+0x2a0..` | `WAKEUP_ZEP_TURRETS` count / name array |
| `+0x2c8`/`+0x2cc..` | `SET_AI_TEAM` count / `{name,team}` records |
| `+0x31c`/`+0x320..` | `SET_AI_ATTACK_RADIUS` count / `{name,radius}` records |
| `+0x398`/`+0x39c..` | `SET_AI_NET` count / `{vehicle,net}` name-pair records |
| `+0x3ec`/`+0x3f0..` | `COMPLETED_ZEPCANNONS` count / `{name,byte}` records |
| `+0x440`/`+0x444..` | `COMPLETED_STOPPOINT` count / `{name,int,bool}` records (cap 10) |
| `+0x4bc`/`+0x4c0` | `ADD_OTHER_TARGET` count / 20-byte string-vector records (cap 10) |
| `+0x4c4`/`+0x4c8` | `REMOVE_OTHER_TARGET` |
| `+0x4cc`/`+0x4d0` | `ADD_OBJECTIVE_TARGET` |
| `+0x4d4`/`+0x4d8` | `REMOVE_OBJECTIVE_TARGET` |
| `+0x4dc`/`+0x4e0` | `START_TAXI` count / name records |
| `+0x508`/`+0x50c` | `SET_HELP_LABEL` string-vector record / label text |
| `+0x510`/`+0x514..` | `STOP_QUEUED_SOUNDS` count / name array (cap 10) |
| `+0x53c`/`+0x540` | `WAKE_ANIM` animation name / target object name |
| `+0x544`/`+0x548` | `SLEEP_ANIM` animation name / target object name |
| `+0x54c` | `WAKEUP_SOUND_GROUP` resolved sound-group handle |
| `+0x550` | `COMPLETED_SOUND_GROUP` handle |
| `+0x554` | outcome class: 1 LOST, 2 WON, 3 INSTANTWIN, 4 INSTANTLOSS, 0 none |
| `+0x558` | HUD completion-notification pending flag |
| `+0x55c` | `INACTIVE_COMPLETION_COUNT` threshold (defaults to `+0x560`) |
| `+0x560`/`+0x564` | `INACTIVE<n>` handle count / heap array of resolved handles |
| `+0x568`/`+0x56c`/`+0x570`/`+0x574` | `DANGER_ZONES_COMPLETION_COUNT` / zone count / ? / per-zone flag bytes |
| `+0x578` | `ANIM_STATE` `{required@0,count@4,records@8}` header |
| `+0x57c` | `COUNTER` record: `{ON_WAKEUP@0, ON_COMPLETE@4, TEST_COMPLETE@8}` triple ptrs |
| `+0x580`/`+0x584`/`+0x588` | `DEDG` group id / remaining threshold / optional generator name |
| `+0x58c`..`+0x5bc` | `TRAVELERS` fields (below) |
| `+0x5c0`/`+0x5c4` | `TIMER_ADJUST`/`ADJUST_TIMER_WHEN_I_COMPLETE` mode (1 SET, 2 ADJUST) / seconds |
| `+0x5c8` | **lifecycle state**: 0 dormant, 1 awake, 2 napping, 3 done |
| `+0x5cc` | per-state float accumulator (seconds) |
| `+0x5d0` | dormant→wake **mission-clock time** (seconds, from `BEGIN_DORMANT` child0) |
| `+0x5d4` | awake **duration** before auto-nap (child1, never spelled in M01) |
| `+0x5d8` | nap **duration** before auto re-wake (child2; also written by `NAP_OBJECTIVE_WHEN_I_COMPLETE`) |
| `+0x5dc` | **mission-clock time** to force state 3 (child3, never spelled in M01) |
| `+0x5e0` | `RESET_TIMER` seconds (consumed on dormant wake) |

Fields `+0x5d4`/`+0x5dc` default to `−1.0f` (never armed by M01 — every one of
M01's 52 `BEGIN_DORMANT` sites spells exactly one float). `+0x144`/`+0x148`/
`+0x14c` and `+0x5e4`/`+0x5e8`/`+0x5ec` are written by parser keys M01 does not
spell and are out of scope.

Mission-level fields used below: `this+0x6f0` mission clock in **seconds**
(`+= frame dt` at `0x46a58e`); `this+0xc44` one-shot "player destroyed" message
flag; `this+0xc48`/`+0xc4c` objective count/array; `this+0xc54` mission-ended
flag; `this+0xc58`/`+0xc5c` won/lost; `this+0xc64`/`+0xc68`/`+0xc6c`
PRIMARY/SECONDARY/TERTIARY complete sounds; `this+0xc70`/`+0xc74` and
`this+0xc78`/`+0xc7c` two win/loss sound pairs (parser names:
`OBJECTIVES_WON/LOST_SOUND`, `MISSION_WON/LOST_SOUND`).

## The update structure (`CZMission::Update`, `0x46a490`)

```
if [0x71c50c] == 0: return 0                 ; mission update disabled
0x46cdf0(0x71b438)                           ; housekeeping on the +0x71b438
                                             ;   sound list (per-node 0x46c900)
0x46c870(0x71b408)                           ; sibling record tick (lazily
                                             ;   resolves +0xc via 0x71b438)
if [0x71c4e0] != 0: 0x480d60()               ; conditional, untraced
0x46c5f0(0x71b468)                           ; mission timer tick: +0x0 += dt,
                                             ;   +0x4 -= dt while running
if 0x46c640(0x71b468):                       ; expired: running && +0x4 <= 0
    if !0x440ad0() && !0x463c00()            ;   && timer+0x14 report-gate clear
       && 0x46c580(0x71b468) > -1.0:         ; remaining > -1.0 s (fresh expiry)
        0x463c30(1, 3.0f)                    ; end sequence — the loss sound
                                             ;   (+0xc7c) while +0xc58 unset
        post text-id 0x1772 (0x59ce40 -> 0x4587d0)
        if !0x440ad0() && !0x4639b0(): post text-id 0x89
        0x46c5c0(0x71b468)                   ; stop the mission timer
this+0x6f0 += frame_dt                       ; mission clock, seconds
if (this+0xc54) return 1                     ; mission ended: nothing runs
0x4639b0() && 0x45b9d0()                     ; pre-step (not traced further)
```

So a mission timer that runs past zero ends the mission through the same
`0x463c30` end sequence the LOST path uses — measured; expiry is what
`MISSION_TIMER`/`RESET_TIMER` arm and `END_TIMER`/`0x46c5c0` forestall.

**Pass 1 — lifecycle timers**, per objective `i` (`0x46a5ce`):

```
if rec->+0x8 == 0                       -> next            ; killed
if rec->+0x10 >= 0 && dep->+0x5c8 != 1  -> next            ; dep must be awake
switch (rec->+0x5c8):
  case 0 dormant:                                          ; 0x46a618
      if +0x5d0 < 0  -> end-check                          ; never auto-wakes
      if +0x14 || +0xc -> end-check
      if clock >= +0x5d0 -> 0x469af0(objs, {i,-1})         ; timed self-wake
  case 1 awake:                                            ; 0x46a677
      if +0x5d4 > 0:
          +0x5cc += dt
          if +0x5cc >= +0x5d4:
              +0x5c8 = 2                                   ; set before the wake call
              r = +0xdc[0] != -1 ? 0x469af0(objs, &rec->+0xdc) : -1
              if r != i: 0x46b160(objs, i, 2)              ; formal nap transition
  case 2 napping:                                          ; 0x46a702
      if +0x5d8 >= 0:                                      ; CF-only test, vs +0x5d4's > 0
          +0x5cc += dt
          if +0x5cc >= +0x5d8 -> 0x469af0(objs, {i,-1})    ; auto re-wake
  case 3 done: -> next                                     ; 0x46a7c1
end-check at 0x46a758 (states 0/1/2 fall through here):
  if +0x5c8 == 1 && +0x5dc >= 0 && clock >= +0x5dc:
      0x46b160(objs, i, 3)                                 ; force "done"
      if +0xdc[0] != -1: 0x469af0(objs, &rec->+0xdc)       ; wake when-I-sleep list
```

`+0x5d0` and `+0x5dc` compare against the absolute mission clock; `+0x5d4` and
`+0x5d8` are per-state durations measured against `+0x5cc` (reset to `0` on
every transition, `+0x5cc += frame_dt` only while in that state). The arm
tests differ by measured comparison mask: `+0x5d4` arms only when `> 0`, while
`+0x5d8` arms when `>= 0` — a `+0x5d8` of exactly `0` still re-wakes, on the
first napping tick once `+0x5cc` has accumulated a frame.

**Pass 2 — condition evaluation and completion**, per objective starting at a
global start index `0x71c128` (normalized `mod count` each tick and never
written anywhere else in the image, so the pass starts at index 0; the modulo
machinery survives although nothing rotates the start):

```
gate, all required (else next):
    +0x10 < 0 || dep->+0x5c8 == 1        ; dep must be awake, same as pass 1
    +0xc != 0                            ; objective itself must be awake
    +0x14 == 0                           ; not already completed

evaluators, first true wins (each runs only if its fields are armed):
    0x469a60  INACTIVE*        -> +0x560 handles, count where +0x24 bit4 CLEAR >= +0x55c
    0x469ab0  DANGER_ZONES     -> nonzero bytes in +0x574[0..+0x56c) >= +0x568
    0x4697a0  ANIM_STATE       -> records whose 0x4ed530(anim) state matches >= header->+0
    0x46b090  TEST_COMPLETE    -> global counter compare (op table below)
    0x465910  DEDG             -> living group members + generator pending <= +0x584
    0x465b40  TRAVELERS        -> subject radius check or group count check

if no evaluator fired — the fallthrough AND-gate (all required):
    +0x560 == 0                          ; no INACTIVE rows
    +0x56c == 0                          ; no DANGER_ZONES
    +0x578 == 0                          ; no ANIM_STATE
    !(+0x580 > 0 && +0x584 >= 0)         ; no armed DEDG
    +0x594 == 0 && +0x58c == 0 && +0x598 < 0   ; no TRAVELERS subject or group
    +0x5c8 == 1                          ; still awake

then, at most once per Update call ([ebp-0x10] zeroed at 0x46a7ee, set on the
first completion — a second objective that would complete this tick is
deferred to the next tick):
    rec->+0x14 = 1 ; rec->+0xc = 0 ; rec->+0x5c8 = 3   ; completion, below
```

An objective with **no armed condition** therefore completes on the first
tick it is awake — the measured behavior behind "the block wakes, so it
completes".

**Completion pipeline** (the `0x46a94c` block, executed in this order):

1. `+0x14 = 1`, `+0xc = 0`, `+0x5c8 = 3`.
2. Play `+0x550` `COMPLETED_SOUND_GROUP` (`0x46cc50`).
3. Play the mission-level class sound: `+0x0==1` → `this+0xc64`, `2` → `+0xc68`,
   `3` → `+0xc6c` (the `PRIMARY/SECONDARY/TERTIARY_COMPLETE_SOUND` fields).
4. If mission not ended (`0x463c00` → `+0xc54`): if `+0x558` pending, when
   `+0x0 != 0` call `0x4a2350(0x71d2a0, +0x0, +0x4)` (post to HUD by
   class+ordinal) and `0x4ad240(+0x4)`; `+0x558` is cleared either way — a
   class-0 record's pending flag drops silently. Then `+0x554 == 3`
   (INSTANTWIN) → `0x463c10(1)` mission WON, byte flag for the instant
   (0.1 s) end delay — this check is **inside** the not-ended guard.
5. `+0x554 == 4` (INSTANTLOSS) → `0x463c20(1)` mission LOST, same — measured
   outside the guard, so it still fires on an already-ended mission while an
   INSTANTWIN at the same point would be skipped.
6. `COUNTER` `ON_COMPLETE` triple (`+0x57c+4` deref) → `0x469dc0` write op.
7. `+0x1c` wake array → `0x469af0` (`WAKE_OBJECTIVE_WHEN_I_COMPLETE` effects).
8. `WARP_VEHICLE` (`+0x154 > 0`): pick `+0x170[rand() % +0x154]`, resolve it
   (`0x4aff10`, `0x4940d0`) and write the chosen destination into the player
   vehicle's pose at `+0x924` region. (Parser key unspelled in M01.)
9. `+0x2c8` `SET_AI_TEAM` records → `0x469e20` per record.
10. `+0x398` `SET_AI_NET` records → `0x46a000` per record.
11. `+0x3ec` `COMPLETED_ZEPCANNONS` records → `0x46a0b0` per record.
12. `+0x440` `COMPLETED_STOPPOINT` records → `0x46a0d0` per record.
13. `+0x31c` `SET_AI_ATTACK_RADIUS` records → `0x469f70` per record.
14. `+0x4bc` `ADD_OTHER_TARGET` records → lazy chain-resolve via `0x466260`,
    then `0x46a1b0` per resolved object: `obj->+0x4c = 1`.
15. `+0x4c4` `REMOVE_OTHER_TARGET` → `0x46a1f0`: `obj->+0x4c = 0`.
16. `+0x4cc` `ADD_OBJECTIVE_TARGET` → `0x46a230`: `obj->+0x4d = 1`.
17. `+0x4d4` `REMOVE_OBJECTIVE_TARGET` → `0x46a270`: `obj->+0x4d = 0`.
18. `+0x4dc` `START_TAXI` records → `0x46a2b0`: vehicle `+0xd4 = 0` (byte).
19. `+0x508`/`+0x50c` `SET_HELP_LABEL` → `0x46a2d0` (below).
20. `+0x510` `STOP_QUEUED_SOUNDS` → `0x5920f0(name)` per queued name.
21. `+0xd0`/`+0xd4` `NAP_OBJECTIVE_WHEN_I_COMPLETE`:
    `0x46b160(objs, +0xd0, 2)` → target `+0x5d8 = +0xd4` → `target->+0x14 = 0`
    (the nap also **clears the target's completed flag**).
22. `+0x94` `KILL_OBJECTIVE_WHEN_I_COMPLETE` → `0x46b130(objs, idx)` per index.
23. `+0x58` `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` → `0x46b160(objs, idx, 3)` each.
24. `+0x5c0`/`+0x5c4` `TIMER_ADJUST`/`ADJUST_TIMER_WHEN_I_COMPLETE`:
    mode 1 → `0x46c510(seconds)` set; mode 2 → `0x46c550(seconds)` adjust.
25. `+0x18` `END_TIMER` → `0x46c5c0` stop the mission timer.

**Post-pass** (`0x46ae98`), only when an objective completed this tick: every
objective's pending `+0x558` on an already-`+0x14` record is flushed to the HUD
(`0x4a2350`/`0x4ad240`, with the same `+0x0 != 0` guard — the flag is cleared
regardless), then count `+0x554 == 2` (WON-class) and
`+0x554 == 1` (LOST-class) records and how many carry `+0x14`. If any WON-class
records exist and **all** are completed → `0x463c10(1)`; if any LOST-class
records exist and all are completed → `0x463c20(1)`. (Measured: a mission
with zero WON-class objectives never wins by this path.)

**End of tick** (`0x46af76`): if `+0xc5c` (lost) → `0x463c30(1, delay)` —
snapshots the clock to `+0xc3c`, sets `+0xc54 = 1`, plays `+0xc7c` (loss
sound) — plus `+0xc74` played separately, `0x46c5c0` stops the mission timer;
delay is `0.1f` when the instant flag was set, `3.0f` otherwise. Else if
`+0xc58` (won) → same shape with `+0xc78`/`+0xc70`. Finally a once-only check:
`this+0xc44 == 0` and the player object `[0x71c298]->+0x934` **strictly above**
the double constant `5.1446` (`0x607c20`) → format `"%s%s"` from
`0x463a50`/`0x4639d0` results and post it via `0x598680`/`0x598710` (the
player-destroyed banner), set `+0xc44 = 1`. What `+0x934` measures on the
player object is untraced.

## Per-directive measured semantics

### `BEGIN_DORMANT` — `[float]`×52 in M01

Parse (`0x467e09`): presence sets `+0xc = 0` and `+0x5c8 = 0` — the block is
**dormant**. Up to four children write `+0x5d0`, `+0x5d4`, `+0x5d8`, `+0x5dc`
(ints are stored bit-for-bit, reals as f32); M01 spells only child0.

Runtime: while dormant the objective does not evaluate (pass-2 gate `+0xc`).
When `+0x5d0 >= 0` and the mission clock reaches `+0x5d0` seconds the
objective **wakes itself** — the full `0x469af0` wake below, including the
dormant-only `RESET_TIMER`/`HIDE_OBJ` effects. `+0x5d0 < 0` disables the timed
wake permanently (the state-0 branch is skipped): the objective stays dormant
until a `WAKE_OBJECTIVE*` list names it. The extra children are measured in
code but never spelled in M01: child1 = seconds awake before the objective
naps itself (state 2, waking its `+0xdc` list), child2 = seconds napping
before it re-wakes, child3 = the mission-clock time at which the objective
forces itself to state 3 (also waking `+0xdc`).

### `TICK_DEPENDS_ON_OBJ` — `[int]`×3 in M01

Parse (`0x4679e1`): `+0x10 = arg − 1`; also clears `+0x588` (the DEDG
generator-name field — an aliased field whose only other writer is DEDG's
optional child2; no M01 site spells both).

Runtime: **both** passes refuse the objective unless `dep->+0x5c8 == 1` — the
dependency must be *currently awake*. While the dependency is dormant,
napping or done, the dependent runs no timers and evaluates no conditions.
When the dependency completes (state 3) the dependent freezes for the rest of
the mission; when it merely naps the dependent pauses and resumes on the
dependency's re-wake. The dependent's own `+0x5d0` wake timer does not run
while its dependency is not awake, so `TICK_DEPENDS_ON_OBJ` delays the
dependent's whole lifecycle, not just its completion.

### `IDENTITY` — `[text,int]`×1, `[text,int,text]`×4 in M01

Parse (`0x468ae1`): child0 `_stricmp` maps `PRIMARY`→`+0x0 = 1`,
`SECONDARY`→2, `TERTIARY`→3; child1 → `+0x4`. Child2, when spelled, is not
read by the parser.

Runtime consumers: the completion sound selects `this+0xc64/68/6c` by
`+0x0 − 1`; the completion HUD post sends `0x4a2350(+0x0, +0x4)` and
`0x4ad240(+0x4)`; the save/load copier (`0x46c310`) carries `+0x14`-style
state and re-posts `0x4ad240(+0x4)`. The class therefore picks the
announcement channel and the ordinal the slot number. Whether the third
child's `MSG_*` label is ever re-read from the retained document
(`this+0x6f4`) is **unknown** (no consumer found).

### Objective-to-objective state directives

All parse their integer children with `dec` into **zero-based** `−1`-terminated
index arrays. `WAKE_OBJECTIVE` and `WAKE_OBJECTIVE_WHEN_I_COMPLETE` are
aliases writing the same `+0x1c` array (measured in A's map at `0x468c0f`/
`0x468c26`); M01 spells only the `_WHEN_I_COMPLETE` spelling, `[int]`×16,
`[int,int]`×2, `[int,int,int]`×1, `[int,int,int,int]`×1.

| Key | Field | Measured effect |
| --- | --- | --- |
| `WAKE_OBJECTIVE(_WHEN_I_COMPLETE)` | `+0x1c` | at own completion, `0x469af0` walks the array |
| `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` | `+0x58` | at own completion, `0x46b160(objs, idx, 3)` per index |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE` | `+0x94` | at own completion, `0x46b130(objs, idx)` per index — `+0x8 = 0`, `+0xc = 0` |
| `NAP_OBJECTIVE_WHEN_I_COMPLETE` | `+0xd0`/`+0xd4` | at own completion, `0x46b160(objs, +0xd0, 2)`, then `target->+0x5d8 = +0xd4`, `target->+0x14 = 0` |
| `WAKE_OBJECTIVE_WHEN_I_SLEEP` | `+0xdc` | `0x469af0`'d when **this** objective auto-naps (`+0x5d4` expiry) or auto-dones (`+0x5dc` expiry) |
| `HIDE_OBJ` | `+0xd8` | when **this** objective wakes from state 0, `objs[+0xd8]->+0x14 = 1` and `objs[+0xd8]->+0x554 = 0` |

There is no bare `NAP_OBJECTIVE`, `KILL_OBJECTIVE` or `SLEEP_OBJECTIVE` key in
the directive string table — `WAKE_OBJECTIVE` alone exists as a bare alias.
`SLEEP_OBJECTIVE_WHEN_I_COMPLETE` and `WAKE_OBJECTIVE_WHEN_I_SLEEP` are
parser-supported but unspelled in M01.

The wake call `0x469af0(objs, idx_array)` walks a `−1`-terminated index array,
**cap 15 processed entries** (skipped entries count toward the cap). Per index
it skips records that are killed (`+0x8 == 0`) or already completed
(`+0x14 != 0`). A record already awake (`+0x5c8 == 1`) gets `+0x5cc = 0` and
**terminates the walk**: the function returns that index immediately, leaving
later list entries unprocessed — re-waking an awake objective resets its state
accumulator and ends the list. The return value is `−1` when the list is
exhausted normally, the last fully-woken index when the 15-entry cap is hit,
or the early-terminated index above; only the auto-nap call site reads it
(`0x46a6e8`), skipping the `0x46b160` transition when it equals the
objective's own index — so a `WAKE_OBJECTIVE_WHEN_I_SLEEP` list whose walk
ends on the objective itself (e.g. a duplicated self-entry the first
occurrence re-woke) leaves the objective awake rather than napping. Otherwise
the record is woken:

* `+0xc = 1`, `+0x5c8 = 1`, `+0x5cc = 0`.
* `WAKEUP_ENEMIES` names (`+0x124`/`+0x128`, cap 10): resolve each in the
  vehicle registry (`0x4aff10`); if found and `veh->+0x945 != 0` →
  `0x4b0f40(veh, 0)`; else zeppelin registry (`0x4bd3e0`); if `zep->+0x5 != 0`
  → `0x4bed40(zep)`.
* `WAKEUP_TURRETS` names (`+0x270`/`+0x274`): `0x4a97b0(0x71d910, name)` per
  name on the turret registry.
* `WAKEUP_ZEP_TURRETS` names (`+0x29c`/`+0x2a0`): `0x465e30(name)` then
  `0x4bef70(obj, 1)`.
* `WAKEUP_GENERATOR` (`+0x118`/`+0x11c`/`+0x120`): resolve the name once via
  `0x451720(0x654170, name)` (the generator table; failure logs `Cannot find
  generator %s`), then `generator->+0x80 += +0x120` — child1 is **added** to
  the generator's pending-spawn count.
* `WAKE_ANIM` (`+0x53c`/`+0x540`): `0x523820(name)` → animation, `0x465e30`
  → target object, `0x4edda0(anim, obj, 0, 0, 0)` — executes the animation.
* `WAKEUP_SOUND_GROUP` (`+0x54c`) → `0x46cc70`.
* `COUNTER` `ON_WAKEUP` triple (`+0x57c` field 0) → `0x469dc0`.
* **Dormant-only effects** (guarded by "prior state was 0", saved before the
  state write): `+0x5e0 > 0` → `0x46c510(0x71b468, +0x5e0)` set the mission
  timer **and start it** (`0x46c5a0`), then `+0x5e0 = 0`; `+0xd8 >= 0` →
  `objs[+0xd8]->+0x14 = 1`, `objs[+0xd8]->+0x554 = 0` (`HIDE_OBJ`: mark the
  named objective completed with no outcome class, hiding it from the
  outcome aggregation). Waking from nap (state 2) or done (state 3) performs
  every other wake effect but not these two.

The `+0x5dc` "force done" transition (`0x46b160` mode 3), the nap transition
(mode 2) and the `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` transition (mode 3) all
run the same helper: `+0xc = 0`, `+0x5c8 = mode`, `+0x5cc = 0`, and play the
objective's `SLEEP_ANIM` — `0x523820(+0x544)` animation on `0x465e30(+0x548)`
target via `0x4edda0(anim, obj, 0, 0, 0)`. `0x46b130` (KILL) writes only
`+0x8 = 0` and `+0xc = 0`: the record stops being processed entirely but is
not marked completed — a killed objective neither ticks nor counts in the
outcome aggregation (it keeps whatever `+0x14`/`+0x554` it had; `+0x14` stays
0 on the paths measured).

`NAP_OBJECTIVE_WHEN_I_COMPLETE [i, t]` (M01: `[int,float]`×27) is the only
in-M01 writer of `+0x5d8`: the target goes to state 2 and re-wakes `t`
seconds later through the state-2 timer. A missing child1 stores the float
`0x3e99999a` (≈0.3 s) and logs `NAP_OBJECTIVE_WHEN_I_COMPLETE has no
nap_time specified.` The measured `target->+0x14 = 0` means napping
**un-completes** a completed target, letting it complete again later (the
`+0x14` re-completion path re-fires every completion effect).

`WAKE_OBJECTIVE_WHEN_I_SLEEP`'s `+0xdc` list fires only on the pass-1
transitions measured above (auto-nap and auto-done). It does **not** fire
when the objective is slept by another objective's `+0x58` list or when it
completes — and in M01 it is never spelled, so the mechanism is
measured-in-code but unused by this mission.

### `INACTIVE<n>` / `INACTIVE_COMPLETION_COUNT`

M01 spells `INACTIVE1`..`INACTIVE18` (the parser accepts `INACTIVE1` ..
`INACTIVE100`) plus `INACTIVE_COMPLETION_COUNT [int]`×9. Each `INACTIVE<n>`
site (`0x469070`) resolves its strings: the first through `0x465e30`, each
subsequent string through a chained member lookup `0x4cc990` on the previous
result — `[base]` → an object, `[base, part, sub]` → the named member
hierarchy inside it (matching F39-E4's measured "node, part, part-state"
triples). Handles are stored in a heap array at `+0x564`, count `+0x560`;
`INACTIVE_COMPLETION_COUNT` writes `+0x55c`, which defaults to `+0x560` (all
rows) when the key is absent.

Evaluator `0x469a60`: `+0x560 == 0` → never fires. Otherwise count the
`+0x564` handles whose object exists (`!= 0`) and whose **`obj->+0x24` bit 4
is clear**, and fire iff `count >= +0x55c`. Measured: an `INACTIVE<n>` row
names a member whose *inactive* state is wanted — the objective completes
when at least the threshold of the listed members no longer carry the active
bit. What sets `+0x24` bit 4 on a given member kind is world code outside
this task's bound (the vehicle/zeppelin code writes it at spawn/despawn
sites); the *count* of cleared members against the threshold is fully
measured.

### `DEDG` — `[int,int]`×8 in M01

Parse (`0x467a0a`): child0 → `+0x580`, child1 → `+0x584`, optional child2 →
`+0x588` (generator name, unspelled in M01). Defaults `−1`/`−1`.

Evaluator `0x465910`: armed iff `+0x580 > 0 && +0x584 >= 0`.
`edi = 0x4658d0(+0x580)` counts members of the circular registry at
`0x71dabc` where `member->+0x91d == 0` (not despawned — the byte the vehicle
code sets on removal) and `member->+0x388 == group` (the member's group/net
id — the same field `SET_AI_NET` writes through `0x475f30`/`0x4bd7a0`, and
the same field TRAVELERS' counting mode tests). If `+0x588` names a
generator, resolve it through `0x451720(0x654170, name)` and add
`generator->+0x80` — the generator's pending-spawn count, the same field
`WAKEUP_GENERATOR` increments. Fire iff `living + pending <= +0x584`.
Unresolved generator names are freed once (with the `Cannot find generator
%s` log) so the error is not repeated.

The per-member predicate `0x465850` is not read-only: on every counted
member it also performs conditional constant stores — `member->+0x318 :=
81000000.0f` when it is below that constant (`0x607c18`), `member->+0x31c :=
−9000.0f` when it is above `−9000.0f` (`0x607c14`), `member->+0x320 :=
+9000.0f` when it is below `+9000.0f` (`0x607c10`). What those member fields
feed is untraced world state; the writes themselves are measured.

Measured: `DEDG [group, remaining]` completes the objective when the
designated enemy group has `remaining` or fewer members still in play (plus
anything its named generator still owes). The acronym expansion is a reading
of the name; the count semantics are measured.

### `TRAVELERS` — `[text,text,text,float,int]`×1 in M01

Parse (`0x467a70`): child0 string → resolved subject (`+0x594`) or pending
name (`+0x58c`); non-string → `+0x598` raw payload. child1 `APPROACHING`
(12-byte `repe cmpsb`) → `+0x59c = 1`, anything else `0`. child2 string →
anchor object (`+0x5a0`) or pending name (`+0x590`); child2 list → explicit
point `+0x5a4`/`+0x5a8` (third component `+0x5ac` unwritten, stays `0`). child3
→ `+0x5b0`, **squared** at parse. child4 (optional) → `+0x5b4` (required
count, default 1). child5 `DELETE_ON_SUCCESS` (18-byte cmpsb) → `+0x5bc = 1`.

Evaluator `0x465b40`: gates on `+0x594 || +0x58c || +0x598 >= 0` else `false`
(a nonnegative `+0x598` alone arms the counting mode).

* Anchor is resolved lazily every call: `+0x590` → `0x465e30` → `+0x5a0`
  (name freed once resolved). Position: `0x4cf200(anchor_obj, &vec3)` if an
  anchor object exists, else the explicit `+0x5a4..+0x5ac` point.
* Subject is resolved lazily: `+0x58c` → `0x465e30` → `+0x594` (freed once).
* **Subject active** (`+0x594` set and `subj->+0x24` bit 4 set): `dist² =
  |subj_pos − anchor|²` (subjects' positions via `0x4cf200`). `+0x59c == 1`
  (APPROACHING): fire iff `dist² < +0x5b0`; `+0x59c == 0` (any other
  spelling): fire iff `dist² > +0x5b0` — strict; equality does not fire.
  On firing, `+0x5bc` deletes the
  subject: `0x4afee0` vehicle hit → `0x47bab0(veh)` (registry removal); else
  `0x4cca30(subj, 0)`.
* **Subject absent or inactive**: falls to the counting path — `+0x598 < 0`
  → return `false` forever (for the `[text,...]` spellings M01 uses, a
  destroyed or never-spawned subject leaves the objective unable to complete
  through TRAVELERS; measured, and consistent with F39-E1's
  dormant-forever population relying on other keys).
* **Counting mode** (`+0x598 >= 0`, reached via non-string child0): iterate
  the registry; `0x4659b0` counts members with `member->+0x91d == 0` and
  `member->+0x388 == +0x598` (group id) that satisfy the radius predicate
  (inside `+0x5b0` when `+0x59c`, outside otherwise, player excluded for the
  deletion predicate `0x465a70`). The per-tick count accumulates into
  `+0x5b8`; `+0x5bc` then deletes every still-matching member. Fire iff
  `+0x5b8 >= +0x5b4` (required count), resetting `+0x5b8 = 0` on success.

So `TRAVELERS` is either "named actor crosses a radius threshold about an
anchor" (the spelled M01 mode) or "at least N members of group `+0x598`
satisfy the radius predicate, cumulatively" (the unspelled counting mode).
`+0x5b4`/`+0x5b8`/`+0x5bc` belong to the second mode; M01's `[int]` child4
sets the required count and its absent child5 leaves deletion off.

### `ANIM_STATE` — `[text,[text,[text],text,[text]]]`×3 in M01

Records `{name, anim_handle, state}` under a `{required@0, count@4, array@8}`
header at `+0x578` (parser: `RUNNING`→2, `EXECUTED`→3, `INVALID`→4).
Evaluator `0x4697a0`: count records where `0x4ed530(anim_handle)` — the
animation's current state — equals the stored state; fire iff
`matched >= header->+0`.

### `COUNTER` / `TEST_COMPLETE` / `ON_WAKEUP` / `ON_COMPLETE`

`0x4693d0` builds the `+0x57c` record `{ON_WAKEUP@0, ON_COMPLETE@4,
TEST_COMPLETE@8}` — each a `0x469440`-parsed `{counter_id@0, op@4, value@8}`
triple from a `[name, opword, int]` child list. `0x469600` interns counter
names into a global registry `{name*, value}` at `0x71c114` (8-byte entries,
count `0x71c110`) — **counters are named global integer registers**, created
zeroed on first mention. Op words: `SET`→0, `ADD`→1, `SUBTRACT`→2,
`TEST_GT`→3, `TEST_GE`→4, `TEST_LT`→5, `TEST_LE`→6, `TEST_EQ`→7, `TEST_NE`→8.

* `0x469dc0` (writes, run at wake/complete): op 0 `= value`, op 1 `+= value`,
  op 2 `= max(counter − value, 0)` (clamped), others no-op.
* `0x46b090` (tests, run every tick): switch on `op − 3` — 3 `>`, 4 `>=`,
  5 `<`, **6 also branches to the `>=` case** (measured: `TEST_LE` evaluates
  as `counter >= value`, identical to `TEST_GE` — an original quirk, not a
  reading), 7 `==`, 8 `!=`. Ops 0/1/2 fall through to `false`.

### `INSTANTWIN` / `INSTANTLOSS`; `WON` / `LOST`

M01's only outcome keys are `INSTANTWIN`/`INSTANTLOSS` (bare, `+0x554 = 3/4`
measured at the parse sites for the sibling keys `WON`→2, `LOST`→1 which M01
does not spell). Runtime measured above: `+0x554 == 3` → `0x463c10(1)` on
completion (immediate mission win, 0.1 s end delay); `+0x554 == 4` →
`0x463c20(1)`. Class 1/2 records do not fire on their own completion — they
feed the aggregation: all WON-class (2) records completed → mission won; all
LOST-class (1) records completed → mission lost. M01's INSTANT* usage
bypasses the aggregation entirely.

### `ADD/REMOVE_*_TARGET` — the target-flag directives

M01: `ADD_OBJECTIVE_TARGET` `[text]`×2 + `[[text,text]]`×2;
`REMOVE_OBJECTIVE_TARGET` `[text]`×1 + `[[text,text]]`×3;
`ADD_OTHER_TARGET` `[text]`×2 + `[[text,text]]`×1;
`REMOVE_OTHER_TARGET` unspelled.

Parse: each argument becomes a 20-byte string-vector record via `0x465fa0` —
a flat `[s]` or nested `[[s,s]]` arg both store the name(s) as record
strings; up to 10 records per list at the measured fields.

Runtime (completion pipeline): each record's names resolve lazily through
`0x466260` — first name via `0x465e30`, subsequent names via `0x4d8cf0` on
the previous result — so `[[base, member]]` means "member `member` of object
`base`". The leaf object then gets its flag:

| Key | Helper | Effect |
| --- | --- | --- |
| `ADD_OTHER_TARGET` | `0x46a1b0` | `obj->+0x4c = 1` |
| `REMOVE_OTHER_TARGET` | `0x46a1f0` | `obj->+0x4c = 0` |
| `ADD_OBJECTIVE_TARGET` | `0x46a230` | `obj->+0x4d = 1` |
| `REMOVE_OBJECTIVE_TARGET` | `0x46a270` | `obj->+0x4d = 0` |

Each helper tries three registries in order: vehicles (`0x4afee0` on
`0x71dab8`), turrets (`0x4a9890` on `0x71d910`), generic objects (`0x4a2850`
on `0x71d338`) — first hit wins; `0x46a230` also skips a null record. The
flags live at `+0x4c`/`+0x4d` on the resolved object and are consumed by the
target-info layer: `0x4a2903`/`0x4a2909` copies them into per-target records
at `+8`/`+9`, and `0x4b406c`/`0x4b4075` into another record at `+0x90`/`+0x91`
— both copies sit next to name-string copies, i.e. target-list entries for
HUD/targeting. So the measured semantic is "mark this world object as an
(other|objective) target on completion"; which icon/label each flag produces
is presentation code beyond this task's bound.

### `COMPLETED_ZEPCANNONS`

`+0x3ec`/`+0x3f0` records `{name, byte}`; `0x46a0b0` resolves the name in the
zeppelin registry (`0x4bd3e0(0x71df80)`) and stores `zep->+0xc = byte`. M01
does not spell the key.

### `COMPLETED_STOPPOINT` — `[[text,int,int]]`×1 in M01

`+0x440`/`+0x444` records `{name, int, bool}` (cap 10). `0x46a0d0` looks the
name up in the global stoppoint table `0x64f610` (`{count@4, ptr_array@8}`,
each element's `+4` a name; case-sensitive inline compare). On a hit it
resolves the stoppoint's instance through the paired table `0x64f614`
(20-byte records, indexed by the matched entry's id) and, when `int > 0`,
calls `0x4319a0(sp, int)` — the stoppoint's own advance/select call — and,
if it returns `!= −1`, `0x4319d0(sp, returned, bool)`. So the directive
forwards `{int, bool}` to the named stoppoint's two-step handler; what the
pair means to a stoppoint is stoppoint code beyond this bound (the arg
forwarding itself is fully measured).

### World-effect siblings (measured at their call sites)

| Key | Consumer | Measured effect |
| --- | --- | --- |
| `SET_AI_NET` (`+0x398` `{vehicle,net}` pairs) | `0x46a000` | vehicle → `0x475f30(veh, net)`; else zeppelin → `0x4bd7a0(zep, net)` (logs `SET_AI_NET: setting vehicle %s to net %s`) |
| `SET_AI_TEAM` (`+0x2c8` `{name,int}` pairs) | `0x469e20` | vehicle → `0x453740` team id, vtable `+8` call, drops old `veh->+0x948`; zeppelin → `zep->+0xdc = 1`, `zep->+0xe0 = team`, `0x4bf030` |
| `SET_AI_ATTACK_RADIUS` (`+0x31c` `{name,float}` records) | `0x469f70` | vehicle → `veh->+0x328 = r²`, `+0x32c = −r`, `+0x330 = r` |
| `START_TAXI` (`+0x4dc` names) | `0x46a2b0` | vehicle → `veh->+0xd4 = 0` (byte) |
| `SET_HELP_LABEL` (`+0x508`/`+0x50c`) | `0x46a2d0` | vehicle → `veh->+0x48 = 0x59cd20(name)` (label id) and a `std::string` at `veh->+0x38` from `0x59ce40(id)` (label text) |
| `STOP_QUEUED_SOUNDS` (`+0x510`/`+0x514`, cap 10) | pipeline step 20 | `0x5920f0(name)` per name |

`SET_AI_TEAM`'s int arg and `START_TAXI`/`COMPLETED_ZEPCANNONS`' byte args
reach concrete fields; the world-side meaning of those fields (AI team id,
taxi flag, cannon state) lives in vehicle/zeppelin code outside this task's
bound.

### Timer keys

The mission countdown timer (`0x71b468`) holds **milliseconds** in `+0x8`
(remaining, resynced from `GetTickCount` deltas by `0x46c610`) and **seconds**
in `+0x4` (a countdown `0x46c5f0` decrements by frame dt while running;
expiry at `<= 0`), with `+0xc` the `GetTickCount` anchor, `+0x10` the running
flag and `+0x14` an expiry-report gate (set → `0x46c640` clamps `+0x4` to 0
instead of reporting). `0x46c510(s)` sets `+0x8 = s·1000` and `+0x4 = s`,
`0x46c550(s)` adds `s·1000`/`s` to them, `0x46c5a0` starts, `0x46c5c0` stops.
On expiry the Update preamble ends the mission via `0x463c30(1, 3.0f)` and
posts text ids `0x1772`/`0x89` — measured above.
Consumers measured: `MISSION_TIMER` (record field, `0x46c510`+`0x46c5a0`);
`TIMER_ADJUST`/`ADJUST_TIMER_WHEN_I_COMPLETE` (`+0x5c0` mode 1/2 → set/adjust
by `+0x5c4` seconds, at completion); `END_TIMER` (`+0x18` → `0x46c5c0` at
completion); `RESET_TIMER` (`+0x5e0` seconds → set+start on dormant wake).
The objective lifecycle timers (`+0x5cc`/`+0x5d0`/`+0x5d4`/`+0x5d8`/`+0x5dc`)
are all **seconds** against the mission clock or `+0x5cc`.

### `COMPLETED_SOUND_GROUP` / `WAKEUP_SOUND_GROUP` / class sounds

`+0x54c` played at wake (`0x46cc70`), `+0x550` at completion (`0x46cc50`).
The `+0x0` class picks `this+0xc64`/`+0xc68`/`+0xc6c` at completion — the
`PRIMARY/SECONDARY/TERTIARY_COMPLETE_SOUND` record fields. The two
`MISSION/OBJECTIVES_WON/LOST_SOUND` pairs land at `this+0xc70`/`+0xc74`
(played at end-of-tick) and `+0xc78`/`+0xc7c` (played inside `0x463c30`).

## The `+0x24` bit-4 "in play" flag, shared across evaluators

`0x469a60` (INACTIVE), `0x465b40` (TRAVELERS subject gate) and `0x4659b0`/
`0x465a70` (TRAVELERS member predicates) all test **`obj->+0x24` bit 4** —
the member is counted/tested only when the bit is clear for INACTIVE, set
for TRAVELERS subject checks. It is the same "object currently in play" bit
the vehicle code clears on despawn (`+0x91d` on registry members is the
kill/remove byte tested by DEDG/TRAVELERS enumeration). The flag's writers
are vehicle/zeppelin/turret code outside this bound; every consumer reading
it for mission purposes is measured here.

## The list-valued shapes the IR cannot carry

`cs_script::ir::Value` has no list variant (`MeasuredArg::is_ir_carriable`
admits only `Int`/`Float`/`Text`). These M01 spellings nest a list and are
therefore not carriable by the current IR — named, not flattened:

| Key | M01 spelling(s) with nested list | Sites |
| --- | --- | --- |
| `ADD_OBJECTIVE_TARGET` | `[[text,text]]` (base + member chain) | 2 |
| `REMOVE_OBJECTIVE_TARGET` | `[[text,text]]` | 3 |
| `ADD_OTHER_TARGET` | `[[text,text]]` | 1 |
| `SET_AI_NET` | `[[text,text]]`, `[[text,text],[text,text]]` | 3 |
| `SET_HELP_LABEL` | `[[text,text],text]` | 1 |
| `ANIM_STATE` | `[text,[text,[text],text,[text]]]` | 3 |
| `COMPLETED_STOPPOINT` | `[[text,int,int]]` | 1 |

Every other M01 key spells only flat int/float/text children. `INACTIVE1`
carries two shapes (`[text]`×2 and `[text,text,text]`×10) — a
`disagreeing_argument_shape`, not a list-carriage problem; both are measured
above as the same handle-chain mechanism with arities 1 and 3.

## Unknowns, kept unknown

1. **`IDENTITY` child2** (the `MSG_*` text at 4 of 5 sites): never read by
   the measured parse; whether the retained document at `this+0x6f4` is
   re-read for it is not established.
2. **`TRAVELERS` `+0x59c` token vocabulary**: `APPROACHING` maps to `1`,
   every other spelling to `0`; whether a distinct third token was intended
   for `0` is unknown — the evaluator treats the two values as "inside" vs
   "outside" radius tests.
3. **`+0x24` bit 4 and `+0x91d` writers**: the world code that sets/clears
   the in-play bit and the despawn byte is not traced — consumers are
   measured, producers are not (vehicle/zeppelin spawn/despawn paths).
4. **`+0xc` vs `+0x8` distinction beyond the measured gates**: kill sets
   both to 0 and is irreversible; no path rewrites `+0x8` after parse —
   `+0xc` is the live/inactive toggle, `+0x8` the permanent removal marker.
5. **World-side effects of the consumers**: `0x4b0f40`/`0x4bed40`
   (wake-enemy calls), `0x4a97b0`/`0x4bef70` (turret calls), `0x475f30`/
   `0x4bd7a0` (AI net), `0x453740`+vtable+8 (AI team), `0x4319a0`/`0x4319d0`
   (stoppoint), `0x59cd20`/`0x59ce40` (help-label id/text) — call sites and
   field writes measured, internals not traced.
6. **`0x71c128` rotation intent**: the pass-2 start index is normalized but
   never advanced anywhere in the image — whether the rotation machinery is
   dead code or fed from elsewhere is unknown; in this build the pass starts
   at index 0.
7. **`+0x570` (DANGER_ZONES field)**: sits between count `+0x56c` and flag
   array `+0x574`; the evaluator uses count+flags only — `+0x570`'s content
   is parser-side (zone handles/names) and its use is not traced.
8. **Malformed-shape behavior**: list children are read without tag checks
   at several sites (e.g. `DEDG` children as raw payloads, index arrays as
   `dec`'d payloads); a non-integer child produces a pointer value, not a
   diagnosed error — observed, not exercised.
9. **DEDG member-field writes**: `0x465850` normalizes `+0x318`/`+0x31c`/
   `+0x320` on every counted group member during evaluation (constants
   `81000000.0f`/`−9000.0f`/`+9000.0f`); what those fields feed is untraced
   world state, and the player object's `+0x934` gating the end-of-tick
   banner is likewise untraced.

## Reconciliation with F39-D / F39-E1..E7

* **F39-E1** measured two `BEGIN_DORMANT` populations (positive arg vs `−1`
  sentinel) disjoint from `INACTIVE` blocks. The mechanism is now measured:
  `+0x5d0 >= 0` = timed self-wake at mission-clock seconds; `+0x5d0 < 0` =
  dormant until an external wake — matching E1's two populations without
  changing any of its counts.
* **F39-E1/F39-E4** measured `INACTIVE<n>` as (node, part, part-state)
  names and `INACTIVE_COMPLETION_COUNT` as a threshold; the evaluator now
  confirms the threshold counts members whose in-play bit cleared, and the
  chained lookup `0x4cc990` confirms the multi-string arity is member
  hierarchy, not three independent names.
* **F39-D**'s bare/argumented directive split survives intact: `INSTANTWIN`/
  `INSTANTLOSS` bare → `+0x554` outcome class; the
  `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`-style list keys land in `−1`-terminated
  index arrays exactly as the flat-walk predicted.
* **F39-E7**'s detached-condition vocabulary is orthogonal — the TRAVELERS
  evaluator's group field (`+0x388`) and the counters are mission-program
  state, not the detach vocabulary E7 measured.

Everything in this document was recovered from code reading; no original
program was run. The `verified_original` level is not claimed anywhere.
