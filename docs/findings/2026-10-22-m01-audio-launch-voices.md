# M01 audio timeline: the launch-cinematic voices (WIP)

Task #1863 (PARITY6-M01-AUDIO-TIMELINE.03). Capabilities: `retail`, `audio`.
Provenance: `static` — read from the installation's own files and the decrypted
engine image; nothing here is `verified_original`. Companions:
`2026-10-22-m01-audio-timeline-voice-laws.md` (the .02 laws this task
implements), `2026-10-22-m01-audio-timeline-sample-identity.md` (which member
each Drydock buffer is), `2026-10-22-m01-audio-timeline-smoke-trace.md` (the
.06 per-second voice trace this task's evidence extends).

**WIP** — the implementation half is in progress; the data half below is
measured and pinned by the task's tests.

## The owner's log, this task's slice

The owner log's set changes this task covers: **0.5** (the other plane's
287,408 B engine loop appears), the **225,000 B one-shot at launch**
(`introscene3.wav`), **10.5** (the 89,056 B one-shot, `introscene4.wav`), and
**13.0** (the player's own engine loop at hand-over). The .02 finding records
that the wall-clock start of scenes 3 and 4 inside the capture is unknown.

## Where the cues live in the data (`generic_intro.zrd`, `ZBD/zrdr.zbd`)

`generic_intro`'s four scenes run in series
(`2026-10-18-parity-m01-lc-cinematic-timeline.md`): `gi_scene1` 0→15.9 s,
`gi_scene2` 15.9→28.3 s, `gi_1stperson` 28.3→38.2 s, `gi_playerdrop`
38.2→40.15 s. The `SOUND` cues are sequence items inside the scene records:

| Cue | Sequence | Anchor (cinematic seconds) |
| --- | --- | --- |
| `SOUND snd_scene1` | `fadefromblack` (of `gi_scene1`) | 0 |
| `SOUND snd_scene2` | scene-2 sequence | 15.9 |
| `SOUND snd_scene3` | `Grahams_sound` (of `gi_1stperson`) | **28.3** |
| `SOUND snd_droplaunch` | `Grahams_sound`, `START_TIME ANIMATION_OFFSET 1.8` | ~30.1 |
| `SOUND snd_scene4` | `gi_playerdrop` sequence | **38.2** |

`snd_droplaunch` has **no group record** in `ZBD/zrdr.zbd`'s `sounds.zrd` —
the name is referenced by the sequence but maps to no member there; where it
resolves (mission-level table or none) is unverified and it stays refused by
name in this task (rule 4).

## The sound groups (`sounds.zrd`, `ZBD/zrdr.zbd`, COMMON set)

| Group | Member | Flags | RANGE | VOLUME |
| --- | --- | --- | --- | --- |
| `snd_devastatorengine` | `devastator.wav` (287,408 B decoded) | LOOPED 3D FREQUENCY SFX | (130, 1020) | 0.8 |
| `snd_scene3` | `introscene3.wav` (225,000 B, 11,025 Hz) | PURGEABLE | — | — |
| `snd_scene4` | `introscene4.wav` (89,056 B) | PURGEABLE | — | — |

M01's devastator-chain airframe records (`pdevastator`, `devastator`,
`rusdevastator`, `wingman`) all resolve `engine_sound` to
`snd_devastatorengine`; `bsfury` resolves `snd_furyengine` (340,032 B — not
the captured size). The scene groups carry no `3D`, `RANGE` or `VOLUME`:
plain one-shots at full level.

## The clock question (recorded, not decided)

Our smoke clock is seconds since Fly Mission; the cinematic is 40.15 s of it.
If the owner's t=0 is offset by ~27.8 s from ours — e.g. the capture's
watch-window begins where `gi_1stperson` starts, or the owner skipped the
first two scenes — the owner instants land on the data's own anchors within
±1 s: 0.5 ≈ 28.3 (scene 3 + the other engine loop), 10.5 ≈ 38.2 (scene 4),
13.0 ≈ 40.15 (hand-over). A constant offset is consistent with the .06
finding's recorded clock-origin unknown; this task anchors each voice to its
measured sequence/scene instant, not to a guessed wall clock.

## What the implementation does (this task)

- The other plane's engine loop (`devastator.wav`): bound at spawn as today,
  but mixed under the measured `snd_devastatorengine` law — RANGE (130, 1020)
  distance gain (the rolloff's `x = (dmax−dmin)/8` knee, the dmax→1.1·dmax
  fade, the 1.1·dmax mute), the bearing pan `dot(src−lis, A)/dist · 0.16`,
  and the Doppler rate `1 − dot(v_src−v_lis, src−lis)/dist/345` — exactly the
  .02 rows. `SoundVolume` (a runtime float) and the listener axis sign stay
  recorded unknowns; the group's own VOLUME 0.8 is applied.
- `introscene3` one-shot at the `gi_1stperson` anchor, `introscene4` at the
  `gi_playerdrop` anchor — played as device one-shots and held so the smoke
  voice trace shows them.
- The player's engine loop is bound at hand-over (the phase's last tick),
  not at spawn: during the cinematic the player aircraft is not yet the
  player in the .02 audible-gate sense.
- Unknowns refused by name: `snd_droplaunch` (no group record), the
  `SoundVolume` float, the listener axis sign, the airframe pitch-scale
  table (`.02` leaves it unread).
