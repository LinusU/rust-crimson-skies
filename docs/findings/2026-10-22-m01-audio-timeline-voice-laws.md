# M01 audio timeline: the engine's volume, pan and rate laws and what starts the voices

Task #1862 (PARITY6-M01-AUDIO-TIMELINE.02). Capabilities: `retail`,
`CS_ENGINE_IMAGE`. Provenance of every row: `static` (read from the decrypted
engine image by hand; nothing here is `verified_original`). The cited bytes are
`[[audio_fact]]` rows of `data/owner-static/engine-facts.toml`, checked against
the image by the ignored test
`accept_parity6_m01_audio_02_cited_rows_match_the_engine_image`. Row names are
quoted as `row.name`. No disassembly is committed. Retail tuning numbers below
are read from `ZBD/zrdr.zbd` (member `player.zrd`, `sounds.zrd`) and
`ZBD/C1/zrdr.zbd`.

Companion: `2026-10-22-m01-audio-timeline-sample-identity.md` (which member each
Drydock buffer is).

## What the engine does (answers to a, b)

The game's sound layer is Zipper's `zSound` (`zsnd_init/play/parm/3d/cd/grp/create.cpp`).
Backend flag `[0x639cb4]`: `0` is the DirectSound software-3D path (the one that
issues `SetVolume`/`SetPan`/`SetFrequency`), `1` is the DS3D hardware branch
(`0x597b40`). The owner's capture shows plain buffer volume/pan/rate calls, so
backend 0 is what the numbers below describe (`zsnd3d.entry`).

### Volume (1/100 dB)

- Gain to 1/100 dB (`zsnd.gain_to_hundredth_db`): `g >= 1 -> 0`,
  `g <= 1/1024 -> -10000`, else `ftol(1000 * log2(g) - 0.5)`: 1000 per octave of
  gain (10 dB per halving).
- A 3D voice starts at `v = gain_dB(desc.volume * SoundVolume * k)`
  (`zsnd3d.base_volume`): `desc.volume` is the sound group's `VOLUME` (0.8 for
  the three engine groups), `SoundVolume` is the Gos named float `"SoundVolume"`
  behind `[0x639e8c]` (1.0 when the name is not registered, `0x591c33`), `k` is a
  per-sound callback factor (1.0 by default).
- Distance `d` to the listener, `dmin`/`dmax` from the group's `RANGE`
  (`zsnd3d.rolloff`, `zsnd3d.fade_zone`, `zsnd3d.mute_beyond`):
  - `d < dmin`: no change. With `x = (dmax - dmin) / 8`, `y = d - dmin`: `y <= x`
    no change; `y > x` adds `1000 * log2(x / y)` (10 dB per doubling of `y`).
  - `dmax <= d < 1.1 dmax`: `v - (3000 + 7000 (d - dmax) / (0.1 dmax))`.
  - `d >= 1.1 dmax`: `-10000` (voice stays alive, silent).
  - Continuity check: at `d = dmax`, `y = 8x`, so the rolloff is exactly -3000,
    where the fade starts: the two pieces were written to join.
- The result is capped at the voice's own gain `handle[+0x24]`, set by
  `0x5978f0(handle, g)` as `gain_dB(g * SoundVolume)` (`zsnd.volume_setter.law`,
  `zsnd3d.volume_cap`). A global mute (`[0x639e80] > 0`, `zsnd3d.mute_predicate`)
  forces -10000.
- Cross-check with the owner's range (-13.2 dB close, -33.8 dB far): an engine
  group has `VOLUME 0.8` and `RANGE (130, 1020)`. `1000 log2(0.8 * 0.5) = -1322`:
  -13.2 dB close range is exactly `0.8` times a total factor of `0.5` ahead of it
  (consistent with `SoundVolume = 0.5` in the capture; the value of that float is
  runtime state, not read here). -33.8 dB is 20.6 dB further down, i.e. `y = 4.2 x`
  (`d ~ 600 m` for the engine range): inside the rolloff, before the fade zone.
  This is a consistency check on the law, not a proof of the capture's geometry.

### Pan (1/100 dB)

`pan = ftol(dot(src - listener, A) / dist * 1600)`, `A` the first vector of the
12-float listener block at `0x639e34` (`zsnd3d.pan_law`, `zsnd3d.pan_constant`):
a source on the `+A` axis is +1600 (16 dB), straight ahead/behind or at the
listener 0. It is `SetPan` at `0x597fcd`. Which world axis `A` is (the listener
orientation setter that fills the block) is not decoded here: the sign of "left"
and "right" is unestablished. The 0..1 setter `0x597840` maps `x` to
`-4000 + 8000 x` (`zsnd.pan_setter.law`), used by scripted sound adjustments,
not by the 3D path.

### Rate

- Setters (`zsnd.pitch_setter.law`, `zsnd.freq_fraction.law`,
  `zsnd.freq_multiplier.law`): every DirectSound rate is clamped to
  4,000..55,200 Hz. `0x597740(handle, m)` sets `round(base) * m`; `base` is the
  descriptor float at `+0x2c` (taken as the sound's native rate; its origin in
  `zsnd_create.cpp` was not decoded).
- Doppler (`zsnd3d.doppler_factor`, `zsnd3d.doppler_apply`): when the caller passes
  the Doppler flag, `factor = 1 - dot(vsrc - vlis, src - lis) / dist / 345`
  (speed of sound 345 m/s, `0x63a594 = 1/345`), `rate = ftol(GetFrequency *
  factor + 0.5)` clamped to 100..100,000 Hz, `SetFrequency` at `0x597f9d`. It
  multiplies the **current** rate, which the aircraft code resets every frame
  with `0x597740`; so the audible rate is
  `round(native) * P * [aircraft+0x68] * doppler` for the engine voice.
- Aircraft voices are updated every frame by `0x4b1c20..0x4b2078`
  (`0x4b1e3b`: `0x597af0(voice, position, velocity, 1)`: Doppler on).

### Aircraft engine and prop voices (`engine.*`, `prop.*`)

Slot 0 is the engine loop, slot 1 the prop loop (`engine.voice_slots`); both are
3D voices at the aircraft position and exist only while the aircraft is the
player or within 2000 m of the player aircraft (`engine.audible_gate`,
`engine.audible_limit`: squared distance < 4,000,000, no square root; otherwise
both are stopped at `0x4b1b47`). With `A`, `B` the throttle fractions
(`engine.throttle_fraction`, `engine.flag_override`, `engine.sideslip_terms`):

```
V = vmin + (vmax - vmin) * A            gain  = master * [0x62ae24] * V
P = pmin + (pmax - pmin) * B            rate  = round(native) * P * [aircraft+0x68]
```

with the pairs read from `player.zrd` (below). `[aircraft+0x68]` is a pitch scale
rolled from the airframe record's sound table (`engine.pitch_scale_roll`); when
slot 0 is empty or finished, a timer re-rolls the table entry every 3-5 s
(`engine.reroll_timer`). The prop voice (`prop.fractions`) uses the speed ratio
`speed / max_speed` (`[+0x934] / [+0x668]`) with the prop pairs and no per-aircraft
scale.

### `player.zrd` tuning (retail values, `config.*` rows give the loader)

Each key is an `(x, y)` float pair.

| Key | Retail (x, y) | Used as |
| --- | --- | --- |
| `engine_sound.min_throttle_volume` | (0.1, 1.0) | volume 1.0 at throttle <= 0.1 |
| `engine_sound.max_throttle_volume` | (1.0, 1.0) | volume 1.0 at throttle >= 1.0 |
| `engine_sound.min_throttle_pitch` | (0.1, 0.6) | `P = 0.6` at throttle <= 0.1 |
| `engine_sound.max_throttle_pitch` | (1.0, 1.0) | `P = 1.0` at throttle >= 1.0 |
| `prop_sound.min_speed_volume` | (1.0, 0.0) | prop silent at speed ratio <= 1.0 |
| `prop_sound.max_speed_volume` | (1.1, 0.5) | prop 0.5 at speed ratio >= 1.1 |
| `prop_sound.min_speed_pitch` | (1.0, 0.65) | `Pp = 0.65` at ratio <= 1.0 |
| `prop_sound.max_speed_pitch` | (1.2, 1.25) | `Pp = 1.25` at ratio >= 1.2 |
| `rattle.sound` | `snd_planeshake` | see below |
| `rattle.volume_range` / `speed_range` | (0, 1) / (1.0, 1.2) | only `speed_range[0]` is read |
| `voiceover_volume_limiter` | 0.4 | not traced here |

So the engine rate multiplier spans `0.6..1.2` (`B` can reach 1.5 through the
sideslip terms: `0.6 + 0.4 * 1.5 = 1.2`) times the per-aircraft scale, times
Doppler. The owner's 6.6-31 kHz is `22,050 * (0.30 .. 1.41)`; the `0.6` floor
gives a bottom of `13.2 kHz * scale`, so a scale near 0.5 and a top scale near
1.17 would be needed. The scale values live in the airframe record's sound table
(offset `+0x78`, 28-byte entries, floats at `+0x14/+0x18`); **those record values
were not read in this task**, so the exact 6.6 and 31 kHz stay explained only in
form, not in number (unknown, rule 4).

## Answers by item

### (a) Callers of SetFrequency / SetVolume / SetPan, and the rate law

Decoded callers (all in `zsnd_parm.cpp` / `zsnd_3d.cpp`): `SetFrequency` at
`0x597624`, `0x597718`, `0x597813` (scripted/aircraft setters) and `0x597f9d`
(3D Doppler); `SetPan` at `0x5978c8` (scripted) and `0x597fcd` (3D);
`SetVolume` at `0x597ec5` (mute), `0x597fd7` (3D result), `0x595c0d/0x595c1a`
(buffer creation: -10000 if muted, else the group volume). `GetFrequency` at
`0x597f57`. The remaining DirectSound-shaped vtable calls in
`zsnd_create/play/cd.cpp` (`0x59283d`, `0x59397f`, `0x594480`, `0x5953e3`,
`0x59b631`, `0x5a10b4`, `0x5996d3`, ...) were listed but not decoded.

### (b) Volume and pan law

Above. Open: the listener orientation vectors (left/right sign) and the value of
the `SoundVolume` float (runtime).

### (c) Wind/ambience loops

- **119,416 B loop** (present t 18-37.5 s, from 83.5 s, absent 40.5-83 s). Two
  looped members decode to this size: `nujetwind.wav` (`snd_jetwind1`, 3D
  `FREQUENCY`, `RANGE (30, 120)`) and `plane_shake.wav` (`snd_planeshake`). A byte
  search of the engine image and of every file under `ZBD/` and `GOSDATA/` finds
  the name `snd_jetwind1` only in the sound table itself (`ZBD/zrdr.zbd`): nothing
  names it, so by name it is not started. `snd_planeshake` is named by
  `player.zrd` (`rattle.sound`) and is the only candidate with a starter: the
  `rattle` consumer at `0x48d21a` (`rattle.consumer`) starts it through `0x45e470`
  when the aircraft is the camera target and `speed >= speed_range[0] * max_speed`
  (retail `speed_range[0] = 1.0`), in the aircraft routine `0x48cf40..0x48d2b7`
  that also plays the crash group and shakes the camera. A launch drop gains
  speed, so "speed above the airframe maximum for the first ~20 s after
  hand-over, then below" would fit the 18-37.5 s window; that is a **hypothesis**:
  how often `0x48cf40` runs, and whether `0x45e470` restarts or reuses a `LOOPED`
  group, were **not decoded**, and the 83.5 s reappearance is unexplained. The
  loop's gate is therefore recorded as "speed vs. `[+0x668]` through the rattle
  consumer", unproven.
- **33,864 B loop** (rate 11,025 -> 22,050 at ~27 s, gated off 26.5-27 and
  32.5-37.5 s). No member decodes to this size (previous finding). The nearest
  engine mechanism is the prop voice, silent at speed ratio <= 1.0 and 0.5 at >= 1.1
  (`prop.fractions`), but its rate multiplier is bounded to 0.65..1.25, which does
  not give the observed x2 step on an 11,025 Hz base. **Unknown**; it is
  not established that this voice is the prop voice.

### (d) The 40.5-48.5 s burst and the 34,408 B pan-57 loop

- **34,408 B loop = `snd_waterfall`.** M01 is `ZBD/C3/M01` (finding
  `2026-10-17-m01-archive-decision.md`); `ZBD/C3/zrdr.zbd` defines the sequence
  `sndseq` = `SOUND snd_waterfall AT_NODE spew` (group `RANGE (200, 1500)`,
  `LOOPED 3D`), and `ZBD/C3/M01/mis_anim.zbd` carries the group name beside the
  `spew` / `spew_puffer` nodes and an `ON_STARTUP` animation `waterrapids01`. (The
  `SOUND_NODE` form with a polygon, which `ZBD/C1/zrdr.zbd` uses for its waterfall,
  is not M01's.) The engine plays a `SOUND ... AT_NODE` entry through the sequence
  handler `0x4ef5d0`, which was **not decoded**, so whether it starts at
  sequence start or is range-gated, and what calls `sndseq` at ~47.5 s, are
  **unknown**. For a node-attached sound the sound layer itself has no distance
  test (`soundnode.update` shows that for `SOUND_NODE`; the 3D law supplies the
  silence at range), so a late first appearance means a late start of the sequence
  or of the node's activity, not the sound layer's range.
- **Pairs of identical sizes in the burst.** Starting a voice takes one pooled
  buffer per play (`0x593320` in `0x5936e0`); nothing in the play path makes two
  buffers for one play. So each pair is two play calls of the same group
  (two triggers, or one trigger repeated), not one sound on two buffers. Which
  events (explosions, hit effects, ricochets: sizes 267,168 / 22,264 / 78,936 /
  18,216 / 220,616 B are the SFX of the previous finding) fire them is **unknown**;
  the ~60 `0x593590`/`0x593b80` call sites were not triaged.

### (e) introscene3 / introscene4

Data-driven. `ZBD/zrdr.zbd` (offsets 1,848,278 and 1,850,241) holds the launch
cinematic's sequences: a `SEQUENCE_DEFINITION` named `Grahams_sound` contains
`SOUND snd_scene3` (no start time, i.e. at the sequence start) and
`SOUND snd_droplaunch` with `START_TIME ANIMATION_OFFSET 1.8`; another sequence
contains `SOUND snd_scene4` at its start. The engine reads these through the
sequence keywords `SOUND`, `SOUND_ADJUST`, `SOUND_NODE` (`sequence.sound_keys`).
The cinematic parent chain (`start_script` -> `gi_scene1` ... with `EVENT_OFFSET`
12.4 / 9.9 / 1.95 and `ANIMATION_OFFSET` 15.9 entries) fixes the scene order but
turning those offsets into seconds after the restore needs the sequence engine's
offset semantics, which were not decoded. **The wall-clock start of scene 3 and
4 stays unknown.**

## What stays unknown (rule 4) and who can resolve it

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| Per-aircraft pitch scale and prop sound of the M01 aircraft | 6.6-31 kHz engine range, the 33,864 B loop | read the airframe records' sound tables (`+0x78`, 28-byte entries) for the M01 aircraft |
| How `0x45e470` plays the rattle group and how often `0x48cf40` runs | 119,416 B loop, t 18-37.5 s and from 83.5 s | trace `0x45e470`, `0x48cf40` callers; compare the sim's speed with `[+0x668]` |
| What calls `sndseq` (`SOUND snd_waterfall AT_NODE spew`) at ~47.5 s; handler `0x4ef5d0` | 34,408 B loop | decode the `SOUND`/`AT_NODE` handler and the M01 sequence call chain |
| Burst triggers 40.5-48.5 s | eight one-shots | triage play call sites by event |
| Cinematic seconds of scenes 3 and 4 | `introscene3/4` | decode sequence offset semantics |
| Listener axis `A`, `SoundVolume` value | pan sign, level offset | read the listener setter; owner setting |

None of the thresholds above was tuned from the +-1 s measured times.
