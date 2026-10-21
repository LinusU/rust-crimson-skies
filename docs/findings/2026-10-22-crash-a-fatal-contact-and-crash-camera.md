# #1853 PARITY4-CRASH-A: the fatal contact, the crash camera and the `Fatal Crash!` message (static)

Date: 2026-10-22. Implementer: `claude-1`. Provenance: **`OWNER-STATIC`** reading of
the owner's decrypted image `crimson.decrypted.exe` (image base `0x400000`,
`llvm-objdump -d`, output in the ignored `private/`) and of the retail
`strings.dll` (RT_STRING resources). Nothing here is an observed behaviour and
nothing is `verified_original`. No gameplay code changed.

Continues `2026-10-21-m01-contact-response-static-locate.md` (#1848) and
`2026-10-21-parity3-camera-modes-01-measured.md` (#1823). Cited instruction
bytes are 18 `[[crash_fact]]` rows (claim `OWNER-STATIC-2026-10-22`) in
`data/owner-static/engine-facts.toml`; test prefix `accept_crash_a_`
(`crates/cs_app/tests/accept_crash_a_fatal_crash_facts.rs`). Addresses are
virtual addresses; `V` is the vehicle object, `player` is `[0x71c298]`.

## (1) What makes a terrain contact fatal

There is **no separate "crash" test** on speed or angle. A contact is fatal only
because the same damage that any contact does drives the health fraction to zero:

* `0x48d2c0` turns the contact severity `s = -dot(normalize(v), n)` into a damage
  pair `(max(300*s^3, 50), max(300*s^3, 50))` (retail `crash` record, #1752 /
  #1848). Plain terrain has no owner (`info+0x24 == 0`), so only the vehicle's own
  damage zones take it (`0x4b3950`, `0x4b3bf0`, #1848 item d).
* After the zones are updated, `V+0x2d0` (the health fraction scaled by `V[0x2cc]`)
  is compared with `0.0f` (`0x48d78b`, row `death.health_test`). `<= 0` jumps to
  `0x48d7c6`, which calls the removal path `0x48b920(V, info)` (row
  `death.call_removal`); otherwise the damage state is refreshed (`0x4b3800`,
  `0x4b1790`) and the plane keeps flying. That is the "light ridge contact, wing
  zone red, plane keeps flying" case of the owner's sequence.
* So the **fatal threshold is zone health sum <= 0**; speed and angle enter only
  through `s^3` (a grazing `s` costs the floor of 50; a head-on `s = 1` costs 300 of
  armor and of health). Both are no-ops while `V+0x384`/`V+0x920` are set (debug
  flags, #1848).
* **Unknown:** the player airframe's zone armor/health totals, and how `0x4b7f80`
  splits one pair between a zone's armor and health, so the number of contacts or
  the `s` at which M01's player dies is not derivable here. The evidence is the
  zone fields `+0x24..+0x30` (#1848). Resolving task: read the Flight Check
  airframe's `armor`/`health` blocks and decode `0x4b7f80`. The owner's Drydock
  crash (item 5) is the ground truth to compare against.

## (2) The detached crash camera

The removal path, when `V` is the camera's target (`cmp esi,[camera+0x150]`,
`0x48ba21`), calls the camera setter `0x42c280` with **mode 5** (row
`camera.set_mode5`). Mode 5 is the "held at a world point, looking at the target"
mode of #1823 (`0x42ce00`):

* **Position (init, once).** `0x42df90` copies the vehicle position `V+0x204` to the
  anchor `0x64ef08..0x64ef10` (row `camera.anchor_copy`), takes the vehicle's heading
  `h` from the matrix (`0x53def0(V+0x180)`), and offsets the anchor:
  `x += sin h * crash_horiz`, `z += cos h * crash_horiz` (rows
  `camera.crash_horiz_x/_z`), `y += crash_y` (row `camera.crash_y`). Then
  `0x42c580(anchor, crash_chord_y, crash_min_elev)` raises it against the terrain
  (row `camera.chord_clamp`, roles of its two arguments **not fully decoded**).
* **Retail numbers** (`camparam.zrd`, default record; no airframe record overrides
  them): `crash_horiz 30.0`, `crash_y 45.0`, `crash_chord_y 1000.0`. The file's
  `crash_elev 40.0` is **not read**: the engine looks up `crash_min_elev`, so that
  field keeps its initial value (unmeasured, #1823). `death_*` keys feed mode 8
  (`0x42e0b0`) and are not the crash camera.
* **Motion.** None: each frame `0x42ce00` copies the anchor to the camera position
  and aims at the vehicle's current `V+0x204` (row `camera.mode5_look_at_vehicle`).
  The camera is a fixed, high point (45 above and 30 along the heading's horizontal direction from the
  impact position, units as the world's) that tracks the falling or wreck only by
  rotating. This matches the owner's "high detached camera looking down on the
  burning plane".
* **HUD/chase replaced.** The camera mode replaces the chase view. Right after the
  message the path calls `0x455800` with zero (row `msg.hud_call`), which sets a flag
  byte on two HUD objects and shows or hides HUD objects; that the zero hides the
  HUD is **inferred** from the code shape, not measured. `0x591f40(1)` follows
  (`0x48ba04`) and touches the sound engine's state; its effect is **unknown**.

## (3) `Fatal Crash!` and `Press Any Key`

* In retail `strings.dll` (language `0x409`) the RT_STRING block 11 holds, with the
  repo's numbering (`cs_formats::pe_resources::string_id`): **162 `Fatal Crash!`**,
  **163 `Press Any Key`**, 164 `Mission`, 165 `Campaign`, 166 `Mission Time:`, 169
  `was shot down`. The names table in the same file lists them as `MSG_CRASH` and
  `MSG_HIT_ANYKEY`.
* **`Fatal Crash!` is shown by the removal path** only when `V` is the player: it
  loads id 162 through `0x59ce40` (row `msg.fatal_crash_id`) and calls the text-line
  function `0x4588e0(text, 5.0f, [0x6eba64], [0x6eba64])` (rows
  `msg.fatal_crash_duration`, `msg.fatal_crash_call`). The sibling death message
  `was shot down` (id 169) uses the same call with the same `5.0f`
  (`msg.shot_down_duration`); another caller passes `1.0f` (`msg.other_call_duration`),
  so the argument is a per-message value, plausibly a duration in seconds.
  **Unknown (not decoded):** the unit of that float, the screen position, the font and
  the colour. `0x4588e0` keeps a cache of line objects keyed by text and the float
  (`0x458350` on `0x6f1ef8`) and stores the two `[0x6eba64]` words at line `+0x104c`
  and `+0x1050`; `0x6eba64` is in `.data` past the file-backed bytes (zero-filled at
  load) and no direct-address writer was found, so its value is **unknown**. The
  owner's screenshot (small, centred, blue, above the plane) is the only evidence
  for position and colour. Resolving task: decode `0x4588e0`/`0x458350` and the
  writer of `0x6eba64`.
* **`Press Any Key` (163) is never referenced** by an immediate id anywhere in the
  image: ids 162, 167, 169, 174 appear as `push imm` before the string loader, 163
  does not. The key-to-continue rule is therefore **not located**; it may be loaded
  from a name-keyed dialog or screen description. The owner's hand reading of the
  string table stands as the only evidence. Resolving task: find the consumer of
  the `MSG_HIT_ANYKEY` text and the state that waits for input after the removal
  path.

## What else the removal path does (player, single-player)

* `0x440ad0` returns `[[0x64f750]]` (multiplayer flag). For single-player the path
  calls `0x46c5c0` on the object at `0x71b468` (row `timer.stop_single_player`): it
  folds the elapsed time into the object and **clears its running byte**, i.e. a
  clock is **stopped at the crash**. This agrees with the owner's observation that
  the debrief Run Time (0:50) is the time of the crash, not later; whether that
  object is the clock the debrief shows is **not decoded**.
* It frees the vehicle's child objects through virtual calls (`+0x3c`, `+0x48`, the
  object at `V+0x948`, `V+0x9bc`), sets `V+0x91d = 1` and `V+0x91e = 0`, and then
  creates an effect object from a template (`0x6379c0`, `0x4edc10`) with the contact
  point and normal (`V+0x8f8`, `V+0x904`): the explosion or wreck, unnamed here. `V+0x91d` is the guard that
  makes camera commands 50..54 inert (#1823), so the camera stays in mode 5.
* The simulation of the plane "stops" for the player by the object being removed
  (`0x48b920`'s body), not by a separate freeze; the exact body past `0x48bc04` was
  not decoded.

## (4) The placeholder target at world (1, 20, 0)

**Unknown.** No `1.0f, 20.0f, 0.0f` triple, no `push 0x41a00000` next to a `1.0f`, was
found, and the target marker's drawing code was not traced in this task. It exists
only as the owner's observation. Resolving task: the target-marker consumer
(`MSG_TRGT_*`, `target_rules`) and what it draws for an empty target.

## (5) The Drydock reference

Supplied by the owner in the task description and **not reproduced here**: crash at
`t = +51.7 s`, player at `(-5055, 324, -4718)`, Run Time `0:50` on the debrief. It is
an observation, not a measurement of mine, and I did not have the capture. It is
consistent with the static reading in one way: the clock stops in the removal path
(so Run Time is the crash time, here 50 s or 51.7 s since the hand-over depending on
the hand-over offset, which is #1655's question).

## Unknown, and the task that resolves each

| item | evidence | resolved by |
| --- | --- | --- |
| player airframe zone armor/health, `0x4b7f80` split | zone fields `+0x24..+0x30` | read the airframe blocks, decode `0x4b7f80` |
| `0x48b920` body past `0x48bc04` (wreck, sim stop) | virtual calls at `0x48b93c..` | decode the removal path |
| message position, font, colour, unit of `5.0f` | `0x4588e0`, `0x458350`, `[0x6eba64]` | decode the text-line object, find the writer of `0x6eba64` |
| `Press Any Key` consumer and the key rule | no immediate reference to id 163 | find the `MSG_HIT_ANYKEY` consumer |
| `crash_min_elev` value (`crash_elev` in the file is unread) | #1823 | none needed unless the clamp matters; `0x42c390` roles |
| placeholder target `(1,20,0)` | owner observation only | trace the target marker draw |
| effect object `0x6379c0` | `0x4edc10` | decode the effect template |
| role of `0x455800(0)` and `0x591f40(1)` | HUD flag, sound state | decode both |
