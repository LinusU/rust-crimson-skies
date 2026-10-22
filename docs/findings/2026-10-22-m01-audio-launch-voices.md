# M01 audio timeline: the launch-cinematic voices

Task #1863 (PARITY6-M01-AUDIO-TIMELINE.03). Capabilities: `retail`, `gpu`,
`audio`. Provenance: `static` plus an owner-command smoke run (a scripted
run, never `human_play`, nothing `verified_original`). Companions:
`2026-10-22-m01-audio-timeline-voice-laws.md` (the .02 laws this task
implements), `2026-10-22-m01-audio-timeline-sample-identity.md` (which member
each Drydock buffer is), `2026-10-22-m01-audio-timeline-smoke-trace.md` (the
.06 per-second voice trace this task's evidence extends).

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

## The owner-command smoke (the task's evidence)

```sh
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=... \
target/release/cs --cs-path <retail> --mission M01 \
  --smoke-seconds 45 --capture-dir private/evidence/PARITY6-M01-AUDIO-03
```

Exit 0, 3,810 frames, 59 checks, all `[pass]`. The trace was first
captured on `26aad982e`, reproduced verbatim after the branch's rebases
(`d6aaa716a`, and again by the reviewer on `0032ad03a`). The review then
fixed one trace-reading fault (below) and re-ran the same command on the
fix commit `3d473ddcf`: the per-second voice set the trace held
— the task's required evidence — beside the owner's instants (our clock
starts at Fly Mission; the ~27.8 s offset question is recorded above):

| Second | Voice set enters/leaves | Owner instant (offset read) |
| --- | --- | --- |
| 1 | `devastator.wav` loop (287,408 B, pan 0.065, rate 30,319) + `introscene1.wav` (354,000 B) | 0.5: the other engine loop |
| 16 | + `introscene2.wav` (277,000 B); `introscene1` drops at 17 | — |
| 29 | `introscene3.wav` (225,000 B); `introscene2` drops | scene-3 cue |
| 39 | `introscene4.wav` (89,056 B); `introscene3` drops | 10.5: the 89,056 B one-shot |
| 41 | second `devastator.wav` loop joins — the player's own at hand-over (40.15 s); `introscene4` drops | 13.0: the player engine |
| 42 | the other plane's loop ends — it crossed the 2,000 m `.02` audible limit and its binding was removed | — |
| 46+ | the player's loop alone at gain 0.80 (`VOLUME` × flat zone) and rate 35,280 Hz | — |

Each one-shot is held exactly its decoded length (`introscene1` 16.0 s at
11,025 Hz mono, `introscene2` 12.6 s, `introscene3` 10.2 s, `introscene4`
2.0 s at 22,050 Hz), so each leaves the second after its cue's scene ends —
the scene voices hand over on the same boundaries the scenes do. The engine
loops' pan/gain/rate columns now carry the `.02` terms (the `VOLUME 0.8`
base, bearing pan inside ±0.16, the Doppler-scaled rates) rather than the
designed flat-zone constants the .06 run recorded.

One trace-reading fault the review found and fixed: the voice trace runs in
`Last`, after transform propagation, but read the `VoiceListenerPose`
`follow_listener` had refreshed a generation earlier in `Update` — so the
player's own coincident loop showed its one-frame displacement as phantom
±0.16 pan swings on some post-hand-over seconds (the device mix itself was
unaffected: `mix_session` reads the pose and the emitter transforms in the
same `Update` pass, so it always heard pan 0). The trace now reads the
player body's transform in its own schedule, and the post-hand-over rows
report the loop centred at pan 0.0 — the value the device heard.

The table lists this task's voices; the trace's seconds also carry the
radio queue's lines — `sparks_1` (3–4), `jack_2` (5–8), `betty_3` (9–12),
`zachary_4` (13–17), `zachary_21` (60) — which are the radio slice's
voices, not this task's.

**No listening claim**: nothing here says the voices are audible or sound
right — `human_review` stays with the owner. Framebuffer evidence from the
same run sits in `private/evidence/PARITY6-M01-AUDIO-03/` (e.g.
`frame_00005_cinematic_start.png`, `frame_02403_cinematic_end.png`,
`frame_02417_cinematic_handover.png`, `frame_02411_first_control.png`).
