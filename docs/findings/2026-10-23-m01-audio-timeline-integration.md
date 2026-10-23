# M01 audio timeline: integration smoke trace beside the owner's log

Task #1757 (PARITY6-M01-AUDIO-TIMELINE, integration step after .01-.06 and the
ambient, sfx-group, rattle, touchdown and ricochet wiring). Capabilities:
`retail`, `gpu`, `audio`. Provenance: `static` plus an owner-command smoke run
(scripted, never `human_play`, nothing `verified_original`). The owner's side
is only the set-change instants and sizes quoted in the task description.
**No audible match is claimed; audible comparison stays with the owner.**

Run (no input, player never moved), exit 0, 5,776 frames, 2 checks, no failures:

```sh
CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path "$CS_GAME_DIR" \
  --mission M01 --smoke-engagement --smoke-seconds 100 \
  --capture-dir private/evidence/1757
```

The voice trace is `audio.voice_trace` / `audio.one_shots` of `report.json`
(under `private/`, not committed; the format is the one of
`2026-10-22-m01-audio-timeline-smoke-trace.md`).

## Clock alignment

Our clock counts from Fly Mission and includes the launch cinematic; the
owner's counts from the checkpoint restore. The one-shot start instants fix a
constant offset of about **27.8 s**:

| Owner event | Owner t (s) | Ours (s) | Ours - owner |
| --- | --- | --- | --- |
| 225,000 B one-shot starts (`introscene3`, 225,000 B) | 0.5 | 28.3 | 27.8 |
| 89,056 B one-shot replaces it (`introscene4`, 89,056 B) | 10.5 | 38.2 | 27.7 |
| second 287,408 B engine loop | 13.0 | 41 (set change) | ~28 |

The offset is inferred from these three instants only (+-1 s per the owner's
note); it is an alignment, not an established fact about either clock.

## Our held voice set, shifted by 27.8 s

| Owner t (s) | Owner set change | Ours | Verdict |
| --- | --- | --- | --- |
| 0.5 | engine loop A + 225,000 B one-shot | `devastator` loop + `introscene3` 225,000 B (from 28.3) | **match** (sizes) |
| 10.5 | one-shot becomes 89,056 B | `introscene4` 89,056 B (38.2) | **match** |
| 12.5 | one voice only | one engine loop only (introscene4 ended, ~40) | **match** |
| 13.0 | player engine (287,408 B loop) in | second `devastator` loop (41) | **match** (sizes; gain/pan/rate not compared) |
| 15.0 | 33,864 B loop in | absent | **differs** (identity unknown) |
| 18.0 | 119,416 B loop in | absent | **differs** (`nujetwind`/`plane_shake` candidates, gating not decoded) |
| 26.5-37.5 | the two loops gated off/on | no gating of those loops | **differs** |
| 40.5-48.5 | burst of 267,168 / 22,264 / 78,936 / 16,192 / 18,216 / 220,616 B | none: no `one_shots` entry between ours 68 and 77 | **differs** (needs a ground contact / hostile fire that the idle run does not produce; the ricochet and touchdown wiring is exercised by tests only) |
| 47.5 | 34,408 B loop, pan 57 (`waterfall`) | `waterfall` 34,408 B loop from 78 | **match** in size and looping; start differs (owner 47.5 + 27.8 = 75.3, ours 78) |
| 83.5 | 119,416 B loop returns | beyond the 97 sampled seconds | **undetermined** |

Radio: our trace also holds `vo_c1-ha-m1_*` lines (sparks_1, jack_2, betty_3,
zachary_4, betty_4-1, then zachary_19, betty_20, britishofficer_24,
zachary_24-1, britishofficer_24-2) at ours 2-17 s and 67-96 s. The owner's
voice list shows no such sizes in his first 10 s; whether the owner's VO sizes
differ (the identity finding lists 78,936 B) or the lines are not played in the
original at those instants is **not established**. The decoded sizes our device
library reports for radio lines are round numbers (see the earlier trace).

## What changed against the baseline trace

The baseline (`2026-10-22-m01-audio-timeline-smoke-trace.md`) held only two
engine loops and a radio line, and its one-shots were invisible. Now the
launch one-shots (`introscene1..4`) and the `waterfall` loop appear, and every one-shot the
device was asked to start is listed in `audio.one_shots` (14 in this run, none
refused).

## Remaining unknowns (rule 4), unresolved by this task

* The 33,864 B loop (15.0 s) and the 119,416 B loop (18.0 s): identity and gating
  rule, and so the 26.5/27/32.5/37.5 s gating.
* The 40.5-48.5 s burst: the destruction/impact callers for 267,168 B and 220,616
  B and the water contact for 78,936 B (see the touchdown/crash and ricochet
  findings); nothing in an idle, unmoved run triggers them.
* Gain, pan and rate laws of the two engine loops against the owner's
  -13.2 dB / 6.6-31 kHz log: ours sits at a constant 30,319 Hz on the first
  loop; the Doppler/speed rate law is not reproduced.
* The owner's log after 83.5 s and its per-voice values are not in this
  task's inputs.
* Our values are the mixer's commands, not read back from DirectSound.

Since parts of the owner's set (33,864 B and 119,416 B loops, the burst, the
engine rate law) are neither identified nor reproduced, the parent task's
"same set of loops with the same gating rules and volume/pan/rate laws" is
**not met**; it is met for the launch one-shots, the engine loops' presence
and the waterfall loop.
