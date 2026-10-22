# M01 audio timeline: our voice set per second beside the owner's log

Task #1866 (PARITY6-M01-AUDIO-TIMELINE.06). Capabilities: `retail`, `gpu`,
`audio`. Provenance: our side is `static` plus an owner-command smoke run (a
scripted run, never `human_play`, nothing `verified_original`). The owner's
side is only the set-change instants quoted in the task description. **No
audible match is claimed.**

## What was added

The owner-command smoke (`docs/PLAYTEST.md`) now records, once per second of
the script clock (frames since Fly Mission at 60 Hz), every voice the audio
session holds: member, decoded 16-bit PCM bytes, looping, final gain (also on
the engine's own dB scale, `0` at unity, `-100` at or below 1/1024, else
`10 log2 g`), pan and rate (member rate x pitch ratio). It is written into
`report.json` under `audio.voice_trace`, with `audio.voice_set_changes` (the
seconds whose member/size/looping set differs from the second before) and
`audio.voice_trace_unobserved`. Code:
`crates/cs_app/src/mission_session/smoke/voice_trace.rs`. Test:
`accept_parity6_m01_audio_06_gain_law_and_set_changes`.

Run (no input, player never moved; the one mode that flies past 90 s):

```sh
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=... \
target/release/cs --cs-path <retail> --mission M01 --smoke-engagement \
  --smoke-seconds 100 --capture-dir private/evidence/PARITY6-AUDIO-06-engagement
```

Exit 0, 5,776 frames, 97 sampled seconds (0..96). The full per-second trace is
committed as `2026-10-22-m01-audio-timeline-smoke-trace.json` (ids and sizes
only, no samples).

## Our voice set, as segments

Voices are always the two `devastator.wav` loops (287,408 B, looping) plus at
most one radio line. Member names are the `ZBD/soundsh.zbd` ids.

| Seconds | Set change | Loop 1 (player): dB / pan / rate Hz | Loop 2: dB / pan / rate Hz |
| --- | --- | --- | --- |
| 0 | empty | | |
| 1-2 | both engine loops start | -3 / 0..0.71 / 30,319 | -100 / 0.41 / 30,319 |
| 3-4 | + `VO_c1-ha-m1_sparks_1` (60,000 B, one-shot) | -3 / -1..0 / 30,319 | -100 / 0.41 / 30,319 |
| 5-8 | line becomes `jack_2` (89,000 B) | -3 / 0 / 30,319 | -100 / 0.41 / 30,319 |
| 9-12 | `betty_3` (88,000 B) | -3 / 0..1 / 30,319 | -100 / 0.41 / 30,319 |
| 13-17 | `zachary_4` (194,304 B) | -3 / -0.71..0 / 30,319 | -100 / 0.41 / 30,319 |
| 18-66 | no radio line | -3 / -1..1 / 30,319 | -100..-2.5 / -0.43..1 / 30,095-35,280 |
| 67-71 | `zachary_19` (210,496 B) | -3 / -0.71..-0.45 / 30,319 | -21..-6.5 / -0.53..0.89 / 32,758-35,280 |
| 72-76 | `betty_20` (113,000 B) | -3 / -1..-0.45 / 30,319 | -25..-17 / 0.14..0.98 / 34,494-35,280 |
| 77-86 | none | -3 / -1..0 / 30,319 | -29..-16 / -0.42..0.98 / 33,858-35,280 |
| 87-90 | `britishofficer_24` (100,000 B) | -3 / -0.71 / 30,319 | -30..-22 / -0.08..1 / 35,280 |
| 91-94 | `zachary_24-1` (167,992 B) | -3 / -0.69..0 / 30,319 | -32..-28 / 0.64..0.92 / 35,280 |
| 95-96 | `britishofficer_24-` (56,000 B) | -3 / -1..-0.71 / 30,319 | -32..-28 / 0.1..0.39 / 35,280 |

(The player loop's rate sits at 30,319 Hz the whole run, 1.375 x 22,050; the
player's engine pitch is not following throttle here, and the owner's 6.6-31
kHz sweep is not reproduced. The pan swings of loop 1 follow the listener
orientation the smoke happens to have; the sign of left/right is itself
unestablished, see `voice-laws`.)

## Beside the owner's log (+-1 s)

The owner's set changed at 0.5, 10.5, 12.5, 13, 15, 18, 26.5, 27, 32.5, 37.5,
40.5 ... 83.5 s. Only those instants reach this task; the instants between
40.5 and 83.5 (the 40.5-48.5 s burst, the 47.5 s waterfall loop) are not in the
text it was given and the capture itself lives outside this checkout, so that
stretch is **undetermined**, not matched or differing. Our set changed at
1, 3, 5, 9, 13, 18, 67, 72, 77, 87, 91 and 95 s.

| Owner change (s) | Ours within 1 s | Verdict |
| --- | --- | --- |
| 0.5 | 1 (both engine loops start) | **match** |
| 10.5 | none (nearest 9 and 13) | differs |
| 12.5 | 13 | **match** (ours: a radio line, owner's content unknown) |
| 13 | 13 | **match** (same instant as above) |
| 15 | none | differs |
| 18 | 18 | **match** (ours: radio line ends) |
| 26.5, 27, 32.5, 37.5 | none | differ |
| 40.5 | none | differs |
| 83.5 | none (nearest 77 and 87) | differs |

Our changes with no owner change within 1 s: 3, 5, 9 (radio lines inside the
owner's quiet 0.5-10.5 s stretch) and 67, 72, 77, 87, 91, 95 (inside the
undetermined stretch up to 83.5, and past it).

Second by second (seconds 0-96, each owner instant widened to +-1 s):
**match** = 0-1, 12-14, 17-19; **differs** = 2-11 and 15-16 (ours holds a steady
set or changes a radio line where the owner's set did not change, or the owner
changed at 10.5/15 and ours did not), 20-40 (owner changes at 26.5, 27, 32.5,
37.5; ours is steady from 18 s to 66 s), 83-84 (owner change at 83.5, none of
ours) and 85-96 (ours changes at 87, 91, 95, the owner's log has no listed
change); **undetermined** = 41-82 (not in the task's inputs).

## What differs in content (members, not just instants)

From `2026-10-22-m01-audio-timeline-sample-identity.md` the owner's set also has
voices ours never plays in this run:

- the 225,000 B and 89,056 B launch one-shots (`introscene3`/`introscene4`
  candidates): none in our trace at 0-96 s of the script clock;
- the 119,416 B loop (18-37.5 s, again from 83.5 s; `nujetwind` or
  `plane_shake`): absent;
- the 33,864 B loop (identity unknown) and the 34,408 B pan-57 loop
  (`waterfall.wav`, ~47.5 s): absent;
- the 40.5-48.5 s burst of one-shots (267,168, 22,264, 78,936, 16,192, 18,216,
  220,616 B): absent. Our smoke cannot see any one-shot (below);
- the owner's two 287,408 B engine loops match ours in size and looping only;
  gain, pan and rate do not (ours: -3 dB and a constant 30,319 Hz on the
  player loop; the second loop is silent for 40 s, then follows its distance).

## Remaining unknowns and limits (rule 4)

- **Clock origin.** Ours is frames since Fly Mission, which includes the 40 s
  launch cinematic. Whether the owner's t=0 is the same instant is not
  established; a constant offset would change every verdict above.
- **One-shots are invisible.** The router hands one-shot cues on without a held
  voice, so gunfire, explosions and ricochets never appear in a second's set,
  and the owner's burst cannot be compared until a device-side tap exists.
- **Radio sizes.** The radio members' sizes read from the device library are
  round numbers (60,000, 88,000, 89,000, 100,000, 113,000, 56,000 B), which a
  decoded ADPCM member would rarely be; whether they are decoded lengths or
  stand-ins is not established, so they are not matched against the owner's VO
  candidates (the VO sizes of the identity finding are 78,936 B and so on).
- **Pan and rate for radio** are not observed: the row carries pan 0 and the
  member's own rate; the device is told neither.
- **The owner log between 40.5 and 83.5 s** and its per-voice gain/pan/rate
  rows are not in this task's inputs; the comparison covers the listed instants
  only.
- **Our values are the mixer's commands**, read from the same inputs the mixer
  uses (loop gain, engine level, spatial law), not read back from DirectSound
  or the output device.
- No claim about what is audible is made; `human_play`/`human_review` are
  unavailable to an agent.

The voice wiring this trace would measure (launch voices #1863, wind loops
#1864, bursts #1865) is not merged, so this is the baseline before those land.
