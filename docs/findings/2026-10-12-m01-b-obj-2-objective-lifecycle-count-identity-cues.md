# M01-B-OBJ-2: `BEGIN_DORMANT`, `INACTIVE_COMPLETION_COUNT`, `IDENTITY` and the sound groups

Date: 2026-10-12. Task: #1359 `M01-B-OBJ-2` (parent #1355 `M01-B-OBJ`). Test prefix
`accept_m01_b_obj_2_`. Capabilities used: `retail` (read of `$CS_GAME_DIR`) and
`CS_ENGINE_IMAGE` (read-only static analysis of `crimson.decrypted.exe`, 2,580,480
bytes). **No original executable was run**; nothing here is `verified_original`.
Everything below is a reading of the executable's code, not an observation of
behavior, and says so where it matters. This note extends M01-B-OBJ-1's finding
(`2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`); addresses
are virtual addresses in the image (PE sections `.text` 0x401000, `.rdata`
0x603000, `.data` 0x619000).

## Where the keys are read

The same loader `OBJECTIVE<N>` loop as M01-B-OBJ-1 reads the four families. In
the serialized record a directive's vector carries `count = elements + 1`, so a
gate of `count > n + 1` means *element n exists*.

| Key | Read at | Stored as |
| --- | --- | --- |
| `BEGIN_DORMANT` | 0x467d74–0x46802a | up to four `f32` at `+0x5d0`, `+0x5d4`, `+0x5d8`, `+0x5dc`; presence flag effect at `+0x5c8`/`+0xc` |
| `WAKE_OBJECTIVE_WHEN_I_SLEEP` | 0x468dc5 | `-1`-terminated index list at `+0xdc` (M01-B-OBJ-1's table) |
| `INACTIVE_COMPLETION_COUNT` | 0x46911d | dword at `+0x55c`; absent → `+0x55c = +0x560` at 0x46914a |
| `IDENTITY` (second string copy, 0x6267f0) | 0x468ae6–0x468b6e | role code at `+0x00`, ordinal at `+0x04` |
| `WAKEUP_SOUND_GROUP` | ~0x468d5e | sound handle at `+0x54c`, name resolved by 0x596120 |
| `COMPLETED_SOUND_GROUP` | ~0x468d9a | sound handle at `+0x550`, same resolver |

Loader initialization (shared, every objective, ~0x467dbf onward): the record is
zeroed, then `+0x5d0 = 0.0`, `+0x5d4 = +0x5d8 = +0x5dc = -1.0` (constant
`0x6032c0`), `+0x5c8 = 1`, `+0x558 = 1`, `+0x14 = 0`, `+0x10 = -1`, `+0xd8 = -1`,
`+0x5e0` holds the `RESET_TIMER` parse. **`BEGIN_DORMANT`'s presence — not its
value — is the dormant signal**: the parse writes `+0x5c8 = 0` and `+0xc = 0`
(0x467d74, `ebx` zeroed there); the absent branch (0x46802a) writes `+0xc = 1`
and leaves the default state 1. Every retail `BEGIN_DORMANT` in M01 carries
exactly one element — `2` on OBJECTIVE1, `-1` on the other 51 — and the `-1` is
significant only as the slot-0 disarm, not as the dormant marker.

## The four timer slots

The per-pass objective loop (0x46a590–0x46a7f0) is gated per objective by
presence and the `+0x10` tick dependency (the linked objective must be awake,
M01-B-OBJ-1). It dispatches on `+0x5c8` through the table at 0x46b07c and then
runs a shared tail. `+0x6f0` is the mission clock — it accumulates the global
frame step `0x9ad744`, which is computed at 0x59c0c0 as `milliseconds × 0.001`
through a timescale, i.e. **seconds**. `+0x5cc` is the per-objective elapsed
timer, written by the state setter 0x46b160 (which also clears `+0xc`).

| Slot | Armed when | Compared against | On fire |
| --- | --- | --- | --- |
| `+0x5d0` (elem 0) | `>= 0.0` (`test ah,1`, 0x46a627) | mission clock `+0x6f0` | `wake([self])` via the shared call 0x46a750 → 0x469af0 — dormant→awake, emits `WAKEUP_SOUND_GROUP`. Extra gates: `+0x14 == 0` (not completed), `+0xc == 0` (not active). |
| `+0x5d4` (elem 1) | `> 0.0` (`test ah,0x41`, 0x46a686) | `+0x5cc += dt` | auto-nap: writes `+0x5c8 = 2` **first** (0x46a6c8), then wakes the `+0xdc` list; unless `wake` returns this objective's index, the setter (0x46b160, state 2) finishes the transition. |
| `+0x5d8` (elem 2) | `>= 0.0` (`test ah,1`, 0x46a711) | `+0x5cc += dt` | `wake([self])` — napping→awake (0x46a738–0x46a755). Also the field `NAP` writes its time into. |
| `+0x5dc` (elem 3) | `>= 0.0` (`test ah,1`, 0x46a771) | mission clock `+0x6f0` | shared tail (state now 1): setter(self, 3) at 0x46a797 **then** `wake(+0xdc list)` at 0x46a79c+. |

Measured quirks preserved in the model:

* **Asymmetric arm tests.** Slot 1 arms only when strictly positive; slots 0, 2
  and 3 arm at zero. Combined with the loader defaults (slot0 `0.0`, slots 1–3
  `-1.0`) a `BEGIN_DORMANT` with no elements would wake immediately; a
  completed `NAP`-time of `0` re-wakes next pass; M01 never exercises either.
* **State is re-read after the case.** The sleep tail tests `+0x5c8 == 1` on the
  *current* state, so an objective that case-0 woke in this pass is immediately
  subject to its (disarmed, `-1`) sleep slot; a `+0x5dc` of `0` would sleep it
  the same pass.
* **The auto-nap self-wake.** `+0x5c8 = 2` lands *before* the `+0xdc` list runs,
  so a list naming the objective itself does not hit the wake routine's
  already-awake early-out; the objective naps only when `wake`'s return is not
  its index. `wake` returns `-1` (list terminator) → modeled `None`, the
  already-awake index on early-out, and the last woken index on a list filling
  the engine's 15-entry window.
* **One pass, one `+0x5cc` accumulation per armed slot.** The timer accumulates
  only while the matching arm test passes — there is no global elapsed
  accumulation.

The loop is **not** gated by the mission-ended flag `+0xc54`; that flag is read
only inside the completion handler's HUD block (through 0x463c00). Whether the
engine keeps running the pass after mission end is outside the measured loop.

## `INACTIVE_COMPLETION_COUNT`

* Parse: element 0's payload stored verbatim at `+0x55c` (no range check).
* Absent: `+0x55c` defaults to `+0x560` — the **resolved** stage count.
* `+0x560` is built by scanning `INACTIVE1`…`INACTIVE100` and resolving each
  stage's condition target (resolver 0x469180); unresolvable stages are dropped
  from the count. `+0x564` is the compacted resolved-target array.
* The checker (0x469a60): `+0x560 == 0` → never fires; otherwise it tallies
  resolved stage targets whose `+0x24` bit `0x4` is **clear** and fires when the
  tally `>= +0x55c`.

**The meaning of `+0x24` bit `0x4` is the condition family's and stays
unmeasured** — the model takes the tally as an input (`flag_clear_stages`) and
never invents it. `ObjectiveRecovery` records the threshold verbatim and stores
the *declared* stage count (distinct `INACTIVE<n>` keys) as
`declared_inactive_stages`: the resolved count's measured upper bound, equal to
it when every stage resolves. Whether M01's stages all resolve is unmeasured
(condition resolution is another family), so the default-threshold values are
structural, not observed.

M01: nine blocks carry the key — O6 `1`/18 declared, O7 `1`/9, O8 `5`/18,
O9 `9`/18, O36 `2`/12, O37 `6`/12, O38 `10`/12, O39 `12`/12, O55 `2`/18 — and
three stage-carrying blocks omit it (O28→9, O54→1, O56→1 declared defaults).

## `IDENTITY`

The objective-path reader (the second copy of the key string, 0x6267f0; parse at
0x468ae6–0x468b6e) requires the element-count gate for elements 0 and 1 only:

* `+0x04` = element 1, the ordinal, verbatim.
* `+0x00` = role code, element 0's text compared case-insensitively
  (`_stricmp`): `primary` → 1, `secondary` → 2, `tertiary` → 3; unmatched → 0.
* **Element 2 is never read on this path.** A second reader at 0x4acd1b (the
  briefing/UI table builder, `0x71d928`) does consume all three elements —
  role, ordinal, message — and is the message identifier's measured consumer.
  The recovery keeps the message verbatim for that consumer but the objective
  runtime ignores it.

M01's five identities: O9 `PRIMARY 1` + `MSG_BRF_NWM1_OBJ1`, O11 `PRIMARY 2` +
`MSG_BRF_NWM1_OBJ2`, O15 `PRIMARY 3` + `MSG_BRF_NWM1_OBJ3`, O18 `PRIMARY 4` +
`MSG_BRF_NWM1_OBJ4`, O21 `SECONDARY 11` with **no** message element — role 2,
ordinal 11 regardless (the objective path needs only two elements).

## Sound groups and the completion-time HUD

Both keys resolve a group name to a handle at load (resolver 0x596120);
`WAKEUP_SOUND_GROUP` → `+0x54c`, `COMPLETED_SOUND_GROUP` → `+0x550`.

* On `wake` (0x469af0 → cue at 0x469ce0 via 0x46cc70): emitted **per woken
  target, after the state writes**, while walking the list — including the
  single-element lists the dormant wake slot and the re-wake slot build.
* On completion: the handler's measured order is `+0x5c8 = 3`, `+0xc = 0`
  (0x46a9xx), then `+0x550` via 0x46cc50 → 0x46caf0, then the role jingle:
  `+0x00` codes 1/2/3 dispatch to the mission-level groups `+0xc64`/`+0xc68`/
  `+0xc6c`; code 0 skips it. Then the HUD block: `+0x558` is a
  notify-once-per-objective flag (set 1 at load). When `+0xc54` mission-ended
  is set the block is skipped and the flag stays set; otherwise the flag
  clears and — only for nonzero role code — the HUD pair (`+0x00`, `+0x04`)
  goes to 0x4a2350/0x4ad240. Role code and ordinal here are `IDENTITY`'s two
  stored fields; the message element plays no part.
* The completion handler does **not** write `+0x5cc`: an unmeasured
  elapsed-reset was dropped from the model for that reason.

`wake` also runs two dormant-only extras, documented and not modeled: it
consumes `+0x5e0` (the `RESET_TIMER` value) once through the HUD-timer pair
0x46c510/0x46c5a0, and follows `+0xd8`, a link that marks another objective
completed. Both fields belong to other field families.

## What is recovered, what is not

`ObjectiveRecovery::read` recovers a `BEGIN_DORMANT` (≤4 numeric elements,
missing slots `-1.0`), `WAKE_OBJECTIVE_WHEN_I_SLEEP` (target list),
`INACTIVE_COMPLETION_COUNT` (first integer element), `IDENTITY` (role text +
integer ordinal, message kept verbatim) and the two sound-group names (first
text element), first occurrence per block, per `EngineField::read`. Each maps to
`EngineDeclaration`; `EngineObjective::loaded` reproduces the load state
(dormant+inactive iff `BEGIN_DORMANT` is present) and `EngineObjectives::advance`
runs the measured timer pass, `wake`, `complete`, `inactive_count_met` and the
counters/notifications described above.

For M01 (`zbd/c1c/m01`, 58 blocks, 358 fields): **166 fields recovered — 52
dormancy, 9 completion counts, 5 identities, 41 sound groups — and zero of the
four owned families unrecovered** (the other 192 fields belong to the
inactive-stage, outcome and still-unmeasured families). The full recovery can
construct `EngineObjectives` for all 58 loaded objectives; the model drives
OBJECTIVE1's measured `[2]`-second wake with `snd_NW1Start`.

`ObjectiveRecovery::program()` **still refuses**, naming the families it does
not measure.

## Unknown, with the evidence that would settle each

1. **`+0x24` bit `0x4` on a stage target** — what clears it is condition
   resolution, M01-B-OBJ-3's family. The count check is measured; the flag's
   meaning is not.
2. **Resolved vs declared stage counts on M01** — whether all 140 `INACTIVE<n>`
   stages resolve (and so whether the declared defaults 9/1/1 equal the
   engine's `+0x560`) needs the resolver's consumers, same family.
3. **The dormant-only wake extras** (`+0x5e0` `RESET_TIMER`, `+0xd8` link) —
   the fields' record keys belong to the HUD-timer and link families.
4. **The pass-rotation origin** for the completion scan (M01-B-OBJ-1 unknown
   #2) — unchanged.
5. **Whether the timer pass runs after `+0xc54` is set** — the flag's measured
   use is the HUD gate only.
6. **Whether the engine's behavior matches this reading when run** — static
   reading only; no original run exists.
