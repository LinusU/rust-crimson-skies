# M01 audio residual: the engine rate law, and what the `+0x78` table really is

Task #1955 (PARITY6-M01-AUDIO-RESIDUAL), one slice: the engine loops' rate/gain
law. Capabilities: `retail`, static analysis of `$CS_ENGINE_IMAGE`. Provenance
`static`; no original run, nothing `verified_original`, **no audible match is
claimed** (that stays `human_review`).

## Correction to the `.02` voice-laws finding

`2026-10-22-m01-audio-timeline-voice-laws.md` says the per-aircraft pitch scale
`[aircraft+0x68]` is rolled from "the airframe record's sound table (`+0x78`)"
and that a scale near 0.5 and one near 1.17 would be needed to reach the owner's
6.6-31 kHz. Reading the loader that fills the table (`0x47a1c0..0x47a74f`, the
`engine_sound` / `cockpit_engine_sound` / `prop_sound` / `damaged_engine_sound`
lookups at `0x47a219`, `0x47a254`, `0x47a280`, `0x47a2ae`) shows:

* `engine_sound` fills `[record+0x6c]` with the group of the key's first name,
  `cockpit_engine_sound` `[record+0x70]`, `prop_sound` `[record+0x74]`.
* The table at `[record+0x78]` (28-byte entries: a string, then floats at
  `+0x14` / `+0x18`) is filled from **`damaged_engine_sound`** (`0x47a2bd` saves
  that lookup; the loop at `0x47a2c7` adds one entry per child; the list form at
  `0x47a433` reads `child[2]`, `child[3]` into the two floats via `0x57a4f0`).
* In retail `vehicle.zrd` exactly one record carries `damaged_engine_sound`
  (`(("snd_damagedengine" 0 1))`, so scale `0 + (1 - 0) * rand`). **`pdevastator`,
  the M01 player airframe, has none**, so its pitch scale is the fixed `1.0f`
  (`engine.pitch_scale_roll`, `0x4b1b33`). The table is the damaged-engine
  sound list, rerolled by `engine.reroll_timer`; it is not a per-airframe pitch
  scale for the normal engine loop.

So for the player's loop `rate = round(native) * P * doppler` with
`P = 0.6 .. 1.0` (up to `1.2` through the sideslip terms), and the scale term
is 1. Arithmetic against the owner's log (unproven, quoted for the next
reader): `11,025 * 0.6 = 6,615 Hz` matches the owner's 6.6 kHz floor, while
`22,050 * 0.6 = 13,230 Hz` does not, so the owner's low end is either an
11,025 Hz-based voice or a Doppler factor near 0.5. The owner's 31 kHz is
`22,050 * 1.41`; `P <= 1.2` leaves a Doppler factor of at least 1.17 (a
closing speed of about 59 m/s at 345 m/s). Which voice the log's rates belong
to is **not established**.

## What changed in the code

Before: the `MeasuredVoice` engine emitter took its pitch from the designed
`EngineSmoothing::DESIGNED_DEFAULT` ramp (`0.7 .. 1.6`, smoothed at 3/s) times
Doppler, which is where the constant 30,319 Hz (`22,050 * 1.375`) of the smoke
trace came from. Nothing in the original has that range or that smoothing.

Now: `cs_sim::audio_events::EngineRateLaw` is the original's pair law
(`engine.throttle_fraction`, `engine.rate_and_gain`), read from `player.zrd`
`engine_sound` by `cs_app::mission_session::engine_rate_law` during
`measure_launch_sound`, installed as `MeasuredEngineRate` by
`install_launch_audio`, and consumed by `smooth_engine_voices`, which sets a
`MeasuredVoice` engine emitter straight to the law's level every fixed tick
(the original does not smooth). Retail pairs: volume 1.0 over the whole
throttle range, pitch 0.6 at throttle <= 0.1 rising linearly to 1.0 at 1.0.
Doppler is still applied by the mixer on top.

Tests: `accept_parity6_m01_audio_10_` in
`crates/cs_app/tests/accept_parity6_m01_audio_10_engine_rate.rs`.

## Still unknown (rule 4)

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| That the flight model's `spool` equals the original's `[aircraft+0x124]` throttle | the pitch the player hears while flying | compare the original-law throttle state with the fixed-wing spool; owner capture |
| Sideslip terms of `B` (`engine.sideslip_terms`, up to +0.5 on `P`) | pitch while manoeuvring | body-frame velocity from the original-law state |
| The `[aircraft+0x947]` override (A = 1.17, B = 1.25) | unknown role | find what sets the byte |
| `[0x62ae24]` fade and the master float `[[0x64f6dc]]` | the owner's -13.2 dB | decode the fade at `0x4b1f9e..0x4b2009` |
| Which voice the owner's 6.6-31 kHz log is | rate comparison | owner log per voice |
| The 33,864 B loop, the 119,416 B loop's gating, the 40.5-48.5 s burst triggers | see `2026-10-23-m01-audio-timeline-integration.md` | unchanged by this slice |
