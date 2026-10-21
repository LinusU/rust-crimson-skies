# M01: examining a site, the 200 m radius and the hand-off (#1754)

Date: 2026-10-21. Capability: `retail` (record read, owner-command smoke); `CS_ENGINE_IMAGE`
for the static reading. Nothing here is `verified_original`.

## The rule (record + static)

* **The record.** `zbd/c3/m01/zrdr.zbd` `objectives.zrd`: `OBJECTIVE2` is
  `TRAVELERS [player, APPROACHING, t_chamber, 200.0, 1]` with `REMOVE_OBJECTIVE_TARGET
  [t_chamber]`, `SET_HELP_LABEL [t_chamber, " "]`, `COMPLETED_SOUND_GROUP
  snd_c1-HA-m1_Zachary_21` and `WAKE_OBJECTIVE_WHEN_I_COMPLETE [3]`. `OBJECTIVE9` (Site #2) is the
  same with the literal point `[-6311, -27, -4611]` and radius 200, `OBJECTIVE12` (Site #3) is
  `TRAVELERS ... shipwreck 200` with `IDENTITY PRIMARY` (the briefing objective).
  `OBJECTIVE19` is a separate far trigger, `t_chamber` at 1500 m (`snd_HI1FarTunnel`).
* **3D, not horizontal.** The subject-mode evaluator (`0x465b40`, finding
  `2026-10-06-m01-lc-directive-c-…`) compares `|subject − target|²` of three-component
  positions with the squared radius, strict `<` for `APPROACHING`. The owner's two triggers
  (126 m/126 m at Site #1; 82 m horizontal / 238 m 3D did **not** fire at Site #2, then 198 m 3D
  did) fit one 200 m 3D radius and no horizontal one.
* **Which point.** The target position for a node anchor is `0x4cf200(node)`, which for an
  ordinary node copies the translation of the node's composed world transform (`0x4cef20`
  builds it by walking the parents; the fast path for a type-5 node with flag `0x80000` copies
  a cached vector at `[node+0x38]+0x84` instead). That is the node **origin**, not the
  bounding-box centre the targeting record shows (`2026-10-21-m01-objective-target-points.md`).
  For `t_chamber` the origin is (-4281.5, 125.2, -4753.2): the owner's trigger sample is 190 m from
  it, the sample 0.3 s earlier 214 m — both sides of 200 m, as they are about the record
  point (178 m / 203 m), so the capture cannot tell the two points apart. Static reading only:
  the `0x4cef20` matrix composition was not traced to the last instruction.
* **One shot.** The objective completes once (`Done`); the flag change is applied once per
  directive emission.

## What changes at completion (implemented)

* `REMOVE_OBJECTIVE_TARGET` clears the site's objective flag: the `MissionGroundTarget` and the
  targeting store's record. `ADD_OBJECTIVE_TARGET` sets it (M01's `OBJECTIVE27` re-flags
  `shipwreck`).
* **The hand-off.** If the cleared site was the selected target, the selection goes to the next
  still-flagged declared site in `targets.zrd` order (Site #1 → Site #2) on the same host
  step. The owner's capture shows exactly this with no key pressed; the *rule* that picks the
  next target is not recovered, the declared order is the designed stand-in (it is the order the
  owner's `E` cycle measured).
* The mission opens with the first declared objective site selected when nothing is: the owner's
  first capture shows Site #1 with no key pressed. Designed, same stand-in.

## Unknown (recorded, not guessed)

* What the original's selection does after Site #2: the owner saw a red `[Destroy] - Truck gun`
  marker; no such target is rostered in our mission, so our selection stays on Site #2 until
  the player changes it. Site #3's successor is unmeasured.
* Whether the examined site still takes part in the `E` cycle (we drop it with its flag).
* `SET_HELP_LABEL t_chamber " "`: the blanked label is not consumed (the retired site is no longer
  the objective marker); no radio line or banner was seen at the hand-off, and our radio plays
  `snd_c1-HA-m1_Zachary_21` through the existing completed-sound cue, whose audibility at that
  moment is unmeasured.
* The map flag / objective list reaction (the briefing map is a separate screen).
* The detection lag the owner estimates (0.1–0.3 s at 100 m/s): our check runs every fixed tick.

## Consumer

`mission_world_observation` step 2c gives every declared ground target a member row at its node
origin; the existing `Travelers` condition and objective lifecycle complete `OBJECTIVE2`;
`TargetFlagLedger` (`mission_session/examine.rs`, host step 6c2) applies the flag directives and the
hand-off through `TargetingSession::select_scripted`.
