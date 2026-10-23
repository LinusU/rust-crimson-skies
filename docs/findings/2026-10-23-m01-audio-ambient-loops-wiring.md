# M01 audio timeline: the rattle and waterfall loops

Task #1864 (PARITY6-M01-AUDIO-TIMELINE.04). Capabilities: `retail`, `audio`.
Provenance: `static` plus an owner-command engagement smoke (a scripted run,
never `human_play`; nothing is `verified_original`). **No audible match is
claimed.** Rules: `2026-10-23-m01-audio-rattle-slot-and-sound-sequences.md`
(#1918) and `2026-10-22-m01-audio-timeline-voice-laws.md` (.02).

## What is wired

* **Rattle (`snd_planeshake`, `plane_shake.wav`, 119,416 B, LOOPED).**
  `crate::audio::RattleSlot` is the keyed voice slot `0x45e470`/`0x45e410`:
  a call starts the group only while the slot holds no handle and stamps the
  expiry to the slot clock; the slot tick stops the voice when the clock is
  strictly greater than the expiry, then advances. `drive_ambient_voices`
  (`FixedLast`, after the cinematic's tick) calls the slot every fixed tick
  the gate holds, then ticks it. Gate (`0x48d21a`): the aircraft is the camera
  target (the launch cinematic is not playing) and `speed >= speed_range[0] *
  max_speed`. The voice is 2D: pan 0, native rate, gain = the group `VOLUME`
  (absent -> neutral 1.0), played on a device loop. Bound only when the
  airframe `mode` is `jet` or `wingman` (the modes `0x489ea0` steps into the
  consumer). `pdevastator`: `mode jet`, `speed_range[0] = 1.0`,
  `fd_speed = 113`.
* **Waterfall (`snd_waterfall`, `waterfall.wav`, 34,408 B, LOOPED 3D, RANGE
  (200, 1500)).** In C3's `waterfalls.zrd` it is `SOUND snd_waterfall AT_NODE
  spew` inside the animation `spew` (`ON_STARTUP`, `EXECUTION_BY_RANGE 600`).
  `anim_range_gate` is the engine's `min <= d^2 < max` test (`0x4ecd20`,
  window `[0, 600^2)` for one value). The loop starts at the first tick the
  player body is inside that window of a `spew` node (one node in the C3
  world: composed position (-4385.73, 145.66, -4872.68)), is started once, and
  is then mixed every tick under the measured 3D law (`measured_mix`: RANGE
  rolloff/fade/mute, bearing pan, Doppler) against the node.
* Both voices' members are declared (designed bus `Impacts`, measured mode
  `Loop`) and delivered from the launch container. They are *not* declared on
  `Environment`: an `Environment` + `Loop` record is bound by
  `bind_spawned_emitters` as the world's ambient emitter, which started them as
  an unplaced pan-`None` loop in the first smoke attempt (seen in the trace,
  fixed).
* The smoke voice trace reads `AmbientVoices::held()`.

## Not wired, by name (plan refusals, rule 4)

* **33,864 B loop (11,025 -> 22,050 Hz):** no member of either archive decodes
  to this size (#1861, owner decision on #1864). Recorded unknown, unwired.
* `nujetwind.wav` (`snd_jetwind1`, the other 119,416 B candidate): nothing
  names it; it is not started.

## Result of the smoke

`target/release/cs --cs-path <retail> --mission M01 --smoke-engagement
--smoke-seconds 100` (no input), exit 0, 5,776 frames: no unplaced extra loops
remain. **The waterfall appears at 78 s** (gain 0.42 rising to 1.0 at 82 s,
pan ~ -0.15, Doppler-shifted rate) as the player flies into 600 m of the
`spew` node, then recedes. The owner's log has it at ~47.5 s on a different
flight. **The rattle never starts in this flight**: the speed gate
`speed >= 113 m/s` did not hold, so the 119,416 B loop of the owner's 18-37.5 s
window is not reproduced.

The owner-command smoke (`--mission M01 --smoke-seconds 45`, no engagement
flag) also passes on this tree: exit 0, 3,894 frames, 66 checks, no failure,
69 framebuffers in `private/evidence/PARITY6-AUDIO-04-smoke` (Git-ignored;
e.g. `frame_00005_cinematic_start.png`, `frame_01982_scene_33s.png`,
`frame_fc00020_flight_check.png`). The change is audio-only, so it adds no
new visual claim.

## Unknowns that remain

| Unknown | Affects |
| --- | --- |
| Whether `[+0x668]` is `fd_speed` (the wingman law reads it that way, designed) and the speed units of `[+0x934]` | whether the rattle gate crosses where the owner's did |
| Why the owner's rattle is absent at 40.5-83 s and returns at 83.5 s | the 119,416 B loop timeline |
| Order of the vehicle step and the slot tick within an engine frame | whether the voice stops one frame earlier or later |
| The group descriptor's default `VOLUME` when `sounds.zrd` has none, and the `SoundVolume` float | the loops' absolute level (the owner's -10 dB is 1.0 x 0.5) |
| Whether a stepped-off animation keeps updating the node sound's 3D mix | the waterfall's pan/gain while out of the 600 m window |
| The camera position the range gate reads while in an external view | the waterfall start under other views |
| The 33,864 B loop | see above |
