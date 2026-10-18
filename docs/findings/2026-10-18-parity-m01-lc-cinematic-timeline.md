# PARITY-M01-LC-MEASURE-CAM-TIMELINE (#1674): how the launch cinematic's scenes follow one another, and which camera variant plays

Date: 2026-10-18. Follow-up to #1662
(`2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`). Capabilities
used: **`retail`** (read-only `$CS_GAME_DIR`, `ZBD/C3/cam_anim.zbd`) and
**static analysis** of the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
sha-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
`objdump -d -M intel`, image base `0x400000`). No original executable ran:
nothing here is `verified_original`, every engine claim is `static`. No timing
was taken from a Drydock run, and the repository holds no timed capture of the
cinematic, so the owner's capture was **not** used as a cross-check.
Test prefix: `accept_parity_m01_lc_measure_cam_timeline_`.

## Verdict

1. **The scenes are chained by waits, and the chain is in the data.** The
   second byte of every event tag (`group`, "measured as a value and unmeasured
   as a meaning" in F20) selects the clock the event's `START_TIME` is compared
   with. With that, `generic_intro`'s four scenes run **one after the other**:
   `gi_scene1` 0 → 15.9 s, `gi_scene2` 15.9 → 28.3 s, `gi_1stperson`
   28.3 → 38.2 s, `gi_playerdrop` 38.2 → **40.15 s** (continuous time).
2. **The player's airframe decides one thing only: whether `gi_1stperson`
   plays.** Balmoral and Warhawk skip it (28.3 → 30.25 s). Which camera
   *variant* plays is not decided by the airframe but by a random draw.
3. **The variant draw is not deterministic.** It reads a 200-entry table of
   `rand()` values; the generator is seeded with `srand(time(NULL))` at start-up
   (or `srand(atoi(arg))` for a `-seed N` command-line option), so the choice
   differs from run to run. Given the seed it is a function of the seed and of
   how many draws earlier consumers took.
4. **The time unit is seconds, and there is no tick rate.** The update step is
   the variable per-frame `game_dt`, at most 0.125 s.

## (d) The unit and the clock (static)

The per-frame update of an animation instance is `0x4ecd20`:

* `instance[+0xb0] += [0x9ad744]` (`0x4ecec0`: `d8 86 b0 00 00 00`), then each
  sequence whose state byte (`[sequence + 0x20]`) is 0 or 1 is stepped by
  `0x4ecbb0` (`0x4ecee8`..`0x4ecef5`). `[0x9ad744]` is `game_dt`.
* F16-F (`2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md`)
  measured the frame clock: `real_dt = (GetTickCount() − last) × 0.001` s,
  `game_dt = scale × real_dt`, clamped to at most **0.125 s** (`0x63b158`,
  `0x63b160 = 1`). This task re-reads the two constants in the image: `0x6075dc`
  is `0.001f` and `0x63b158` is `0.125f`
  (`..._image_instructions_are_the_ones_the_finding_read`).
* So `START_TIME` words are **seconds of game time**, advanced in **variable
  steps** (one per rendered frame, at most 0.125 s each). There are no fixed
  ticks. The `-freq F` debug option (F16-F) forces a fixed `1/F` step; it is not
  the shipping policy.
* An event runs on the **first update whose clock has reached its wait**, so it
  runs late by a part of one frame, never early. Each wait in a chain adds its
  own lateness (≤ 0.125 s). The times below are the continuous-time values.

`CALL_ANIMATION`'s instance start zeroes the child's clock (`0x4ed600`,
`xor ebx,ebx` at `0x4ed607`, store at `0x4ed686`); the call chain from the
`CALL_ANIMATION` executor to that start was not walked in full, so "a child's
clock starts at 0 when it is called" is a read of the start routine, not of
the whole path.

## (a) The sequencing rule (static, then confirmed on the data)

The step function reads the event's tag byte 1 (`0x4ecc04`: `mov eax,[esi+0x34]`
/ `xor ecx,ecx` / `mov cl,[eax+1]`), and compares the event's `START_TIME`
(`[event + 8]`) with a clock; the event runs when the clock is **not below** it:

| group | clock | where | bytes |
| --- | --- | --- | --- |
| 1 | the instance clock `[instance + 0xb0]` (absolute) | `0x4ecc3f` | `d9 87 b0 00 00 00 d8 58 08` |
| 2 | the sequence clock `[sequence + 0x24]` | `0x4ecc2c` | `d9 46 24 d8 58 08` |
| 3 | the time since the previous event of the sequence ran `[sequence + 0x28]` | `0x4ecc19` | `d9 46 28 d8 58 08` |

An event runs only after the one before it in the stream has run (the
sequence's event pointer `[sequence + 0x34]` advances by the event length,
`0x4ecc9c`), so a wait blocks everything behind it in the same sequence.
After an event runs, `[sequence + 0x28]` is set to 0 (`0x4ecc96`); it grows by
`game_dt` once per update from the next update on (`0x4ecbe4`..`0x4ecbf9`), and
only once the first event of the sequence has run. Further events whose wait is
already met run in the same update (`0x4eccba`). Group 2 does not occur in the
`generic_intro` family.

The other executors involved:

| opcode | executor | what it does here |
| --- | --- | --- |
| 24 `CALL_ANIMATION` | `0x4eb6b0` | starts the named child instance and, unless event word `+0x2e` has bit `0x10` set (`0x4eba0e`: `f6 45 2e 10`; clear in every `generic_intro` call), **does not wait** for it |
| 25 `STOP_ANIMATION` | `0x4ebc80` | stops the named child (not walked beyond its name match) |
| 22 `CALL_SEQUENCE` | `0x4eb570` | finds the sequence by name in the instance's table (stride `0x40`, name at `+0`) and, if its state is 3, sets it to 0: the `0x303` sequences are the dormant ones, the flag-0 sequences run when the instance starts |
| 31 `IF` | `0x4ec080` | skips to the matching `ELSE`/`ELSEIF`/`ENDIF` when false; the skip does not track nesting (`0x4ec3bd`..`0x4ec3e3`), and the data has no nested `IF` |

### The timeline of `generic_intro` (measured on the data, replayed by `cs_app::cinematics::intro_timeline`)

`start_script` (flag 0, runs at the instance start), events in stream order,
group in brackets:

| event | wait | runs at | note |
| --- | --- | --- | --- |
| re-parent `camera1`, `CALL_ANIMATION` `apzep_engines_start`, `letterbox`, `gi_scene1` | 0 (1) | 0 | `apzep_engines_start` and `letterbox` are never stopped by the cinematic |
| `STOP_ANIMATION gi_scene1` | **15.9 s, group 1** | **15.9** | absolute: on the instance clock |
| `CALL_ANIMATION gi_scene2` | 0 (1) | 15.9 | same update as the stop |
| `STOP_ANIMATION gi_scene2` | **12.4 s, group 3** | **28.3** | 12.4 s after the previous event ran |
| `CALL_SEQUENCE check_balmoral` | 0 (1) | 28.3 | |

`check_balmoral`: `IF node 5 (player_balmoral) active` → `CALL_SEQUENCE
drop_planes`, else `CALL_SEQUENCE check_warhawk`. `check_warhawk`: `IF node 6
(player_warhawk) active` → `CALL_SEQUENCE drop_planes`, else `CALL_ANIMATION
gi_1stperson` (group 1, wait 0), `STOP_ANIMATION gi_1stperson` (**9.9 s,
group 3**), `CALL_SEQUENCE drop_planes`. `drop_planes`: `CALL_ANIMATION
gi_playerdrop` (wait 0), `STOP_ANIMATION gi_playerdrop` (**1.95 s, group 3**).

| scene | camera/player tracks (#1662 numbering) | starts | stops | stop − start |
| --- | --- | --- | --- | --- |
| `gi_scene1` | `gi_cam1`/`b`/`c`/`d` (0–3), one by draw | 0 | 15.9 | 15.9 |
| `gi_scene2` | `gi_cam2`/`b`/`c` (4–6) by draw, `gi_pfighter1` (7) | 15.9 | 28.3 | 12.4 |
| `gi_1stperson` | `gi_player1` (8), `gi_cam3` (9) | 28.3 | 38.2 | 9.9 |
| `gi_playerdrop` | `gi_player2` (10), `gi_cam4`/`b` (11, 12) by draw, `gi_pfighter2` (13) | 38.2, or 28.3 for Balmoral/Warhawk | 40.15, or 30.25 | 1.95 |

* **Total length: 40.15 s** (15.9 + 12.4 + 9.9 + 1.95), or **30.25 s** when the
  player's root is `player_balmoral` or `player_warhawk`. #1662's "nominal sum
  40.3 s" added the track lengths (15.9333 + 12.4375 + 9.95 + 1.9667); the
  scenes are *stopped* at the stop times, before their tracks end, so 40.3 is
  not the length.
* Each track is cut at its stop: `gi_cam1` is cut 0.033 s before its end,
  `gi_cam2` 0.04 s, `gi_cam3` 0.05 s, `gi_cam4` 0.017 s.
* `gi_scene2` opens the Pandora's launch doors with a `CALL_ANIMATION`
  (`start` 5.0, group 1) on **its own** instance clock, so at 15.9 + 5.0 =
  **20.9 s** of the cinematic.
* The camera track's local time `u` is its **own scene instance's clock** minus
  the segment start, so each scene's camera starts at its scene's call time:
  `gi_cam2*` runs on the camera at cinematic times 15.9 → 28.3, and so on.
* Each wait can run up to 0.125 s late (see above), and the late amount carries
  into the next group-3 wait (it measures from the frame the previous event
  ran), but not into a group-1 wait (absolute). The stop of `gi_scene2` is thus
  at 28.3 + (0 … 0.25) s, the end at 40.15 + (0 … 0.5) s in the worst case.

## (b) Which scene the player's plane selects (static + data)

`IF` condition word `0x2000` (`0x4ec1c8`: `f6 c7 20`, i.e. bit `0x2000`) calls
`0x4e8d60(instance, event[+0x10])`, which returns the object bound to
`Nodes[index]` of the record (`instance[+0xe8] + 44 * index + 0x28`; 0 when
unbound), then tests `[object + 0x24] & 4` (`0x4ec1e5`: `f6 40 24 04`).
`OBJECT_ACTIVE_STATE` (`0x4cca30`) sets that bit for state 1 (`83 4e 24 04`,
`0x4ccaba`) and clears it for state 0 (`83 66 24 fb`, `0x4ccac7`) for the
node types it handles by default. An unbound node, or a clear bit, makes the
`IF` false. The record's `Nodes` table lists `player_balmoral` as node 5 and
`player_warhawk` as node 6.

`ZBD/C3/gamez.zbd` has **no** node named `player`, `player_balmoral` or
`player_warhawk`, so these are bound from outside the world file. The airframe
table (`2026-10-06-m01-lc-player-airframe-source.md`) names the scene root
`player_<airframe>` for each of the eleven airframes (row 2 `player_balmoral`,
row 10 `player_warhawk`), and the loader at `0x47ece1` calls `0x47fd50` with the strings `pbalmoral` and
`player_balmoral` (that routine was not walked). So the `IF` reads "the player flies the
Balmoral / the Warhawk": the two airframes that skip the first-person shot.
**Any other airframe, including the Devastator row the M01 default is
inferred to be (#1580), gets the long cinematic.**

## (c) The camera variant draw is seeded from the clock (static)

* The `IF` executor with condition bit 0 (`0x4ec09c`: `f6 c3 01`) reads
  `table[cursor]`, the 200-entry float table at `0x9fce20`, advances the
  cursor `[0x72836c]` modulo 200 (`0x4ec0a1`..`0x4ec0bf`), and is true when the
  value is **≤** the event's `+0x14` float (`0x4ec117`: `fcomp`, `test ah,0x41`).
  The cursor and the table are in the zero-initialised tail of `.data`, so the
  cursor starts at 0, and nothing but these readers writes it.
* The table is filled by the loop at `0x4ee41a`: `call ebx`, with `ebx =
  [0xa20350]`, the import slot of **MSVCRT `rand`**, multiplied by
  `0x38000100` (1/32768 as a float, the constant stored at `0x728358`), so
  entries lie in [0, 1). The filling code is the tail of the animation reset
  `0x5239b0`, which is called from the start-up function (`0x4a7f2e`), from the
  end of the animation shutdown `0x523d70` (`0x523e0d`) and from `0x5b5746`.
* The start-up function seeds `rand` with **`srand(time(NULL))`** (`0x4a742a`:
  `push 0; call [time]; push eax; call [srand]`), before the fill at `0x4a7f2e`
  (same function, higher address). Its command-line parser accepts **`-seed
  N`** (string at `0x629904`) and calls `srand(atoi(N))` (`0x4a7662`,
  `0x4a7669`), between the two. Two other places seed with a constant
  (`0x4df1d0`: `srand(0x8ea91836)`, followed by `srand(time(NULL))` in the
  same routine; `0x44e035`: `srand(0x9b3a9ce2)`, with a `srand(time(...))` at
  `0x44e5eb` whose routine was not established to be the same). They are not
  on the path of the start-up fill; whether one of them precedes a later
  refill (below) without a time re-seed was not established.
* Consequences, with the probabilities assuming a uniform draw and one draw per
  `IF` (`cs_app::cinematics::intro_timeline::variant_chains`, read from the
  data; thresholds `0x3e4ccccd` = 0.2, `0x3e99999a` = 0.3):

| scene | thresholds | variants (track blocks) | probability |
| --- | --- | --- | --- |
| `gi_scene1` | 0.2, 0.2, 0.2 | `gi_cam1` (0), `gi_cam1b` (1), `gi_cam1c` (2), `gi_cam1d` (3) | 20 %, 16 %, 12.8 %, 51.2 % |
| `gi_scene2` | 0.3, 0.3 | `gi_cam2` (4), `gi_cam2b` (5), `gi_cam2c` (6) | 30 %, 21 %, 49 % |
| `gi_playerdrop` | 0.3 | `gi_cam4` (11), `gi_cam4b` (12) | 30 %, 70 % |
| `gi_1stperson` | — | `gi_cam3` (9) always | 100 % |

  (#1662 omitted `gi_playerdrop`'s draw.) A run draws 1–3 values for scene 1,
  1–2 for scene 2 and 1 for the drop, from the one shared cursor. The cursor
  is also advanced by the random-range event at `0x4e90aa`, so which table
  entries the intro reads depends on how many draws came before it. **The
  original is therefore not reproducible from the data alone**; a
  reimplementation that wants "the" original camera cannot have one. It should
  draw from its own generator with these thresholds, and say so.

## What is still unknown, what it affects, who resolves it

| unknown | affects | resolver |
| --- | --- | --- |
| what starts `generic_intro` and where t = 0 lies relative to the Flight Check screen, the mission load and the first drawn frame | the wall-clock moment of every cut | #1663 |
| which `player_<airframe>` root has bit `0x4` set while the cinematic runs (the code that sets it for the chosen airframe is not traced; if every root were active the first-person shot would never play) | whether `gi_1stperson` plays | #1663 |
| the player's airframe in M01 (the data states none; Devastator is inferred) | 40.15 s versus 30.25 s | #1580 / #1663 |
| the number of draws taken before the cinematic | which variant, given the seed | an original run |
| the frame deltas of an original run | each cut by up to one frame late | an original run |
| `STOP_ANIMATION`'s effect beyond naming the child (what it does to the camera's last pose, the letterbox, the engines) | the frame at each cut | #1663 / #1665 |
| group 2 and a group-3 wait on the first event of a sequence | nothing in `generic_intro` (no such event); the replay refuses both | — |
| whether the refill of the random table at the animation shutdown (`0x523e0d`) runs between M01's load and its cinematic, and which seed state it then sees (see the constant seeds above) | which table the intro reads; the start-up table is time-seeded | an original run |
| the field of view; the letterbox slide-in timing | not this task | #1665, #1664 |

## Superseded statements

* `2026-10-06-f20-event-grammar.md`: "the tag's second byte … is measured as a
  value and unmeasured as a meaning". It is the **wait clock selector** (above)
  for events of an animation sequence. `DecodedEvent::group` still carries the
  value; `cs_app::animation::events::sequence_duration` still treats every
  `START_TIME` as an absolute offset and so under-reports a chained sequence.
  That function is not changed here; `intro_timeline` is the replay that honours
  the groups, and a follow-up should move F20's playback onto it.
* `2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`: the rows
  "how `gi_scene1`, `gi_scene2`, `gi_1stperson`, `gi_playerdrop` follow one
  another", "the time unit and the tick rate", "whether the random generator …
  is seeded" and "how the player's plane … changes the scene" are answered here;
  the nominal sum 40.3 s is replaced by 40.15 s / 30.25 s.
* `2026-10-18-m01-launch-end-pose.md`: `intro-clock` and `branch-choice` are
  answered here (the drop starts at 38.2 s, or 28.3 s for Balmoral/Warhawk, in
  continuous time).

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- \
  accept_parity_m01_lc_measure_cam_timeline_ --include-ignored
```

Three synthetic members author every event byte (the wait groups, the seven
refusals, the variant probabilities); two retail members replay the real
`generic_intro` for four airframe roots and read the real variant chains; the
image member pins the instruction bytes this note quotes.

## Files

* `crates/cs_app/src/cinematics/intro_timeline.rs` (new): `RecordView`,
  `measure_intro_timeline`, `IntroTimeline`, `SceneSpan`, `variant_chains`,
  `VariantChain`, `TimelineError`, `TIMELINE_UNKNOWNS`,
  `MAX_UPDATE_DELTA_SECONDS`.
* `crates/cs_app/src/cinematics/mod.rs`: `pub mod intro_timeline;` (wiring).
* `crates/cs_app/tests/campaign/parity_m01_lc_measure_cam_timeline.rs` (new)
  and `tests/campaign/mod.rs` (`mod …;`, wiring).
