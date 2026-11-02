# M01 wingman at the hillside: the AI blow against the follow law (#2262)

Date: 2026-11-02. Task #2262, follow-up of #2260
(`2026-11-02-m01-wingman-after-the-ai-ground-blow.md`). Implementer:
`sonnet-1`; not independently reviewed. Static claims are `static` (claim id
`OWNER-STATIC-2026-10-29`), read from the owner image with
`llvm-objdump -d -M intel`; the original executable was not run and nothing is
`verified_original`. The port measurements below are port behaviour only.

The byte windows the answer rests on are re-read by
`accept_m01_wingman_hillside_windows_match_the_image`
(`crates/cs_app/tests/campaign/m01_wingman_hillside.rs`, ignored, needs
`CS_ENGINE_IMAGE`).

## Answer

**Option 3 for the blow, and an unported stick law behind the follower; no
production code changes.** Neither question 1 nor 2 turns up a gap the port can
close from the bytes in hand:

1. **The AI blow is applied as the bytes say.** The block is the same for both
   seats up to the factor: a non-player takes `ai_groundblow * groundblow_mag`
   (`0x48c317`, 0.5 x 10 = 5) with **no dot test and no backfire branch**, the
   player takes `(T . B) * groundblow_mag` (so a player who is not commanding
   gets almost no torque), both steer the velocity toward the nose by the same
   `1 - f(2 dt amount)`, and a non-player is skipped only for `V+0x358 == 4`
   or `now < V+0xac` (`0x48c22b`). All of that is what `ground_blow` and
   `ground_blow_gated` already do (`2026-10-29-groundblow-assist-model.md`,
   `2026-11-01-groundblow-gates.md`). So an AI aircraft is torqued by every
   hillside hit and the no-input player is mostly only re-aimed: the
   asymmetry the port shows is in the bytes.
2. **The follower is not suspended while the blow runs, and has no
   re-acquire gain the port lacks.** `0x41c270` (the AI step) is gated by
   `V+0xcc` alone; mode 4 reaches the follower `0x41e760` with only the
   `V+0x358` guard inside it (3: fly up, 4: no control). Neither the step nor
   `0x41b560` reads `V+0xac`; the only `V+0xb4` read is the stick-gain
   attenuation (`now < V+0xb4` multiplies the three axis gains by `[0x6034a8]`),
   and no site gives `+0xb4` a non-zero value (`2026-10-29-groundblow-world-segment-sweep.md`
   section 6). The call the step makes for modes 0/4/1 at `0x41c2d5` is a bare
   `ret` stub (`0x41afd0`). The follower writes `+0x100/+0x108/+0x10c`,
   which `0x41c01e`.. copies to `+0x114/+0x11c/+0x120`: the force block reads
   those as **roll, pitch, yaw** (`0x48caea`, `0x48cb07`, `0x48cbb7`), the same
   block the ground blow adds its torque to. Stick and blow therefore sum
   every tick, in the original as in the port.
3. **What the port does differently is the stick, not the blow.** The port's
   follower aims `pursuit_command` (bank/pitch/yaw gains, all labelled
   **designed**) at a station point. The original's `0x41b560` is a normalised
   bang-bang law (`2026-10-18-m01-wingman-player-leader.md` section 4) with the
   goal's `y` clamped to the vehicle ceiling `+0x314` and the world floor
   `[0x71c3f0]` (`0x41b57e`..`0x41b5f1`), the 80 m push, and the decoded
   `+0x100 -> roll`, `+0x108 -> pitch`, `+0x10c -> yaw` mapping, none of which
   the port flies. How strongly the original's stick fights a 5 x B torque is
   therefore unmeasured, and the port's loss of station says nothing about the
   original's wingman.

## What the port does (measured, headless, retail M01)

`accept_m01_wingman_formation_betty_engages_and_closes_on_the_player` with a
diagnostic print every 0.5 s (not committed), 120 Hz ticks:

| run | Betty's gap at t=4560 / 4800 / 5340 | final gap |
| --- | --- | --- |
| as main (Betty's blow on) | 36 m / 134 m / 179 m | 157 m |
| Betty's `groundblow_mag` = 0 (player's untouched) | 73 m / 43 m / 32 m | 46 m |

With her blow off Betty holds station (closest 22 m, final 46 m under the
118 m spawn gap), so the whole opening of the gap is the AI blow acting on a
designed stick that cannot counter it. In the "on" run Betty overshoots the
slot at 36 m (no 80 m push in the port), the designed yaw saturates at -1 for
~2 s from t=4740 and she is lifted 75-85 m above the player; the divergence
begins at t=4620 (38.5 s), a second after the probe first meets the hillside.
(The player's own altitude differs between the two runs by ~8 m from t=4380;
the cause was not chased and does not change the comparison.)

## Consequence

* Left as is: the blow law (bytes agree), `groundblow_mag = 0` in the
  scenery-collider fast-body flights (the blow still lifts az6/h110 over the
  ring; the test asks whether scenery is solid), and the #2260 closest-approach
  pin. They are not re-pinned.
* Unresolved, with the content it affects: whether the **original** wingman
  holds station through the hillside. It depends on `0x41b560`'s stick law
  against the blow, which is decoded (player-leader finding section 4) but not
  ported, and on three unknowns the port flies as designed: the sign of the
  leader basis rows (slot side/height/ahead), the direction of the 80 m push
  (`[ebp-0x40]` is read before any store), and `def+0x260`. Resolving task:
  port the decoded stick law with the measured `+0x114/+0x11c/+0x120` mapping
  (filed as a follow-up with this task), then re-run the hillside flight.

## Instruction bytes cited

Machine-readable windows are the `WINDOWS` table of the test above; the blow
windows are the ones in `2026-10-29-groundblow-assist-model.md`.
