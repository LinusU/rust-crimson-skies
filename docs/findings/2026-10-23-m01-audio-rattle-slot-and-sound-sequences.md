# M01 audio timeline: the rattle slot helper, the SOUND/AT_NODE handler and the START_TIME clocks

Task #1918 (unblocks #1864, PARITY6-M01-AUDIO-TIMELINE.04). Capabilities:
`retail`, `CS_ENGINE_IMAGE`. Every row is `static` (read from the decrypted engine
image; nothing is `verified_original`), recorded as `[[audio_fact]]` rows of
`data/owner-static/engine-facts.toml` (21 new rows, claim
`OWNER-STATIC-2026-10-22`). The ignored test
`accept_parity6_m01_audio_04_cited_rows_match_the_engine_image` re-reads the image
and compares every cited byte. No disassembly is committed. The retail data
statements (the `waterfalls.zrd` and `generic_intro` sequences) were read with the
project's `unzbd` extraction into `private/` and are **not** guarded by a test
(see "Not guarded").

Companion: `2026-10-22-m01-audio-timeline-voice-laws.md`, whose sections (c), (d)
and (e) asked these three questions.

## (a) The rattle: one voice, held by a keyed slot

`0x45e470(ref, key, group, ttl, pos*, vel*)` is a **keyed voice slot**, not a
fire-and-forget play:

1. It finds or creates the record for `key` (argument 2) in the map at `0x71b1e8`.
2. Only when the record holds no handle (`[rec+0x10] == 0`) does it start the
   group through the 2D play `0x593590(group, 1.0f)` and keep the handle. A record
   that already has a handle is **not restarted**.
3. It then stamps the record's expiry, `[rec+0x14] = slot_clock + ttl`, where the
   slot clock is `[0x71b1f8]`.

The slot clock and the reaping are in the frame routine: `0x4a09a4` calls
`0x45e410(dt)` with `dt = [0x9ad744]` (the frame delta that also steps the
animation clocks). With `[0x71d558] == 0` it calls `0x45e360` on every record
(stop the voice with `0x593dc0` and clear the handle when the clock is **strictly
greater** than the expiry) and *then* advances the clock by `dt`.

The rattle consumer (`0x48d272`, gate in `rattle.consumer`) passes the key
`[0x71c338]`, the group `[0x71c334]` (`snd_planeshake`), `ttl = 0` and null
position/velocity. Consequences, all from the code:

- While the gate (`speed >= speed_range[0] * [+0x668]`, aircraft is the camera
  target) holds each frame, one voice plays: started at the first true frame,
  never restarted, its expiry re-stamped to the current clock every frame.
- When the gate stops holding, the voice is stopped by the **second** slot tick
  after the last stamp (tick one sees `clock == expiry`, not greater; tick two
  sees `clock = expiry + dt`).
- The slot is 2D: no pan or distance law applies to the rattle.
- Whether the voice is a loop is the group's `LOOPED` flag (previous finding: the
  `snd_planeshake` member is looped); a non-looped group would play once and stay
  held.

**How often the consumer runs.** The consumer sits in the force routine `0x48c470`,
called from `0x48e580`, which `0x489ea0` reaches for vehicle modes **0 and 4** only
(`jet` is mode 0; `wingman` is 4; the jump table is `vehicle.update_table`).
`0x489ea0` is called once per vehicle from the vehicle-list step `0x4897c0`
(registered as a per-frame scene callback by `0x4739e8`), which steps a vehicle
only when `[+0x944]` is nonzero. So the consumer runs **once per rendered frame for
each live vehicle in mode 0 or 4**, and a vehicle that is not the camera target is
not heard (the gate tests `[[0x64ef78]+0x150]`). The player's aircraft is a jet-mode
vehicle unless its record sets another `mode`; that mapping was not read for the M01
aircraft (see Unknown).

What this changes for the 119,416 B loop (t 18-37.5 s and from 83.5 s):
`snd_planeshake` is a **sustained single voice** held for exactly as long as the
speed gate is true, plus two frames. The earlier hypothesis (speed above the
airframe maximum for about the first 20 s after hand-over) is now the *only*
statically supported reading, but whether the simulated speed `[+0x934]` crosses
`speed_range[0] * [+0x668]` at those instants is a runtime comparison: the 83.5 s
reappearance is still unexplained, and the 119,416 B size is still shared with
`nujetwind.wav`, which nothing names.

## (b) The SOUND handler `0x4ef5d0` and what starts `sndseq`

`0x4ef5d0` is a **compile-time** step of the sequence loader (it has no sound
play call). For a `SOUND` entry it appends a 0x1c-byte event to the sequence
buffer: type byte `1`, default time-mode byte `1`, offset `0.0`; reads `NAME`
(truncated to 31 characters; `Missing Sound NAME` is an error), resolves the group
through `0x4eedb0` into the word `[event+0xc]` (zero is `Missing Sound Effect`, a
loader error), parses `START_TIME` through `0x4ef270` and `AT_NODE` through
`0x4ef7d0` into the word `[event+0xe]`. `AT_NODE` takes node names (optionally
`GLOBAL`/`LOCAL` scoped; `INPUT_NODE` is `-200`, `MAIN_ROOT_NODE` `-100`). The
play itself is the executor already listed in the call-site table
(`sequence_sound`: `0x4e7564`, `0x4e7585`, `0x4e9ba5`): 3D at the named node,
else 2D.

**What gates `sndseq`.** In the retail data `sndseq` (`SOUND snd_waterfall AT_NODE
spew`, M01 `ZBD/C3/zrdr.zbd`, `waterfalls.zrd`) is the second sequence of the
animation `spew` (`ANIMATION_NAME spew_water`, `ACTIVATION ON_STARTUP`,
**`EXECUTION_BY_RANGE 600`**). The string `sndseq` occurs once in the extracted
`C3` archive: **nothing calls it by name**; it is not `ON_CALL`. Its start is the
animation's, and the animation is range-gated by the engine:

- `EXECUTION_BY_RANGE` sets `[anim+0x9c] |= 2`. With one value it stores
  `[anim+0xa4] = 0` and `[anim+0xa8] = value`, then squares `[anim+0xa8]`
  (`0x51edb0`); two values set minimum and maximum.
- The per-step routine (`0x4ecd20`, gate at `0x4ecd83`) computes the squared
  distance from the animation's node `[anim+0x6c]` to the camera position
  (`[0x9fd184..0x9fd18c]`, helper `0x4ec540`) and steps the animation's sequences
  only for `min <= d^2 < max`; otherwise it skips the step and sets a skip counter
  from a rotating counter (`[0x727fd0]`, 0..10), which spreads the retest of far
  animations over the next frames.

So `snd_waterfall` (looped, 3D, `RANGE (200, 1500)`) **does not start until the
camera is within 600 world units of the `spew` node**, and then it starts at the
first step with offset `0`. That fits a late first appearance (about 47.5 s in the
owner's log) as "the flight reaches 600 units of the first `spew` node at about
that time": the cause is the engine's animation range gate, not the sound layer.
Whether the M01 trajectory is within 600 units of a `spew` node at 47.5 s was not
checked (positions of the `spew` nodes are in `M01/mis_anim.zbd`'s node tree).

## (c) START_TIME offset semantics (introscene3/4)

`0x4ef270` parses `START_TIME <word> <number>`: `ANIMATION_OFFSET` stores mode
`1`, `SEQUENCE_OFFSET` mode `2`, `EVENT_OFFSET` mode `3`; the latter two are
rejected as the first event of a sequence. The sequence step `0x4ecbb0` fires the
current event when a clock has **reached** the offset:

| Mode | Clock compared | Meaning |
| --- | --- | --- |
| 1 `ANIMATION_OFFSET` | `[anim+0xb0]` | seconds of frame time since the animation started stepping |
| 2 `SEQUENCE_OFFSET` | `[seq+0x24]` | since the sequence started |
| 3 `EVENT_OFFSET` | `[seq+0x28]` | since the previous event of the sequence fired (reset to one `dt` when it fires) |

Both clocks advance by the frame delta `[0x9ad744]` (the engine's own frame
time; it is the delta the voice-slot clock above uses). An event without
`START_TIME` has mode 1, offset 0 and fires at once. **Events of one sequence run
strictly in order**: an event whose clock has not reached its offset blocks the
events behind it.

Applying that to the launch cinematic (`generic_intro`, read from `ZBD/zrdr.zbd`):

| Event (sequence `start_script` unless noted) | Offset | Fires at (s after `start_script` starts) |
| --- | --- | --- |
| `CALL_ANIMATION gi_scene1` | none | 0 |
| `STOP_ANIMATION gi_scene1` | `ANIMATION_OFFSET 15.9` | 15.9 |
| `CALL_ANIMATION gi_scene2` | none | 15.9 |
| `STOP_ANIMATION gi_scene2` | `EVENT_OFFSET 12.4` | 28.3 |
| `CALL_SEQUENCE check_balmoral` | none | 28.3 |
| `gi_1stperson` called (scene 3, `Grahams_sound`: `snd_scene3` at animation offset 0, `snd_droplaunch` at `ANIMATION_OFFSET 1.8`) | none | 28.3 unless `player_balmoral` or `player_warhawk` is active, in which case scene 3 is **skipped** |
| `STOP_ANIMATION gi_1stperson` | `EVENT_OFFSET 9.9` | 38.2 |
| `CALL_ANIMATION gi_playerdrop` (scene 4, `snd_scene4` at its start) | none | 38.2 (28.3 for the two skipping aircraft) |

So, **statically**: `snd_scene3` at about 28.3 s and `snd_droplaunch` at about
30.1 s after `start_script` starts, `snd_scene4` at about 38.2 s, for the aircraft
that run scene 3. Scene 1 starts `snd_scene1` and scene 2 `snd_scene2` at 0 and
15.9 s. These are seconds of the engine's frame clock, **not** seconds of the
owner's log: the offset between `start_script`'s start and the log's origin (the
restore) is not established, and which aircraft M01 uses decides whether scene 3
plays at all. Treat the table as relative spacing: scene 3 to scene 4 is 9.9 s,
scene 1 to scene 3 28.3 s, droplaunch 1.8 s after scene 3.

## Unknown (rule 4) and who can resolve it

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| Whether the M01 player aircraft's vehicle `mode` is 0 (jet) and the simulated speed crosses `speed_range[0] * [+0x668]` at t 18-37.5 s and from 83.5 s | the 119,416 B loop | read the M01 aircraft's `mode`; compare `[+0x934]` of the sim with the gate; explain 83.5 s |
| Whether the flight is within 600 units of a `spew` node at about 47.5 s | the 34,408 B pan-57 loop | node positions from `M01/mis_anim.zbd` against the camera path |
| Whether a `SEQUENCE_DEFINITION` with neither `ACTIVATION` nor a `CALL_SEQUENCE` starts with its animation (assumed from the loader; the default activation word was not read) | `sndseq` | read the sequence activation default in the loader |
| Origin of the cinematic clock against the log's time, and the M01 aircraft (decides if scene 3 plays) | `introscene3/4` wall-clock | the restore/hand-over call that runs `start_script`; the M01 aircraft definition |
| Whether `CALL_SEQUENCE` and `CALL_ANIMATION` start at the same step or the next | +-1 frame of the table above | read the executors of event types for those calls |

## Not guarded

The retail data statements (`sndseq`, `spew`, `EXECUTION_BY_RANGE [600]`, the
`generic_intro` chain and values) come from the project's own `unzbd`
extraction and are not asserted by a test; a later task can pin them with a
`CS_GAME_DIR` test. The Rust test guards the cited image bytes and the table shape.
