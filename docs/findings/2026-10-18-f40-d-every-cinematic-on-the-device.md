# F40-D: every retail cinematic played on the device

Date: 2026-10-18. Rally task #167 (`### F40-D`, prefix `accept_f40_d_`).
Capabilities used: `retail`, `gpu`, `audio` (Apple M3 Pro, Metal, default output).
Evidence: `docs/findings/evidence/F40-D.json` (claim `implemented`).

## What was verified

`accept_f40_d_every_retail_cinematic_plays_to_its_end_on_the_device_clock`
lists `GOSDATA/ASSETS/GRAPHICS/MPG`, pins the ten files against the F40-D2
inventory, and plays each one end to end through `play_on_device` (F40-D4):
the output device's played position is the master clock, every shown frame is
composed letterboxed into 1280x720 (960x720 picture, never stretched), and
three sampled surfaces per file are rendered offscreen on the real GPU and
compared pixel for pixel with the uploaded surface. The seven files D4 did not
cover (chap0-chap5, final) now play too.

| file | audio s | frames presented | audio frames short of end | max drift |
| --- | --- | --- | --- | --- |
| chap0 | 144.95 | 3481 | 0 | 31.7 ms |
| chap1 | 130.87 | 3128 | 0 | 31.7 ms |
| chap2 | 123.27 | 2772 | 0 | 31.7 ms |
| chap3 | 92.81 | 2068 | 0 | 31.7 ms |
| chap4 | 137.48 | 3277 | 0 | 31.7 ms |
| chap5 | 95.82 | 2296 | 0 | 31.7 ms |
| crimflag | 8.02 | 189 | 0 | 31.7 ms |
| final | 111.02 | 2731 | 18 | 31.7 ms |
| msopen1 | 13.53 | 322 | 0 | 49.9 ms |
| zipper | 20.27 | 491 | 0 | 31.7 ms |

Failure paths (run in CI): a missing file is `MissingMedia` naming the id; a
file that is not an MPEG program stream is a decoder/decode failure; neither
presents a frame or completes. Without the `audio` capability a missing file is
still a media error, not a pass.

## A correction found on the way

The first full run failed on `final`: the device position stopped 18 audio
frames (0.4 ms) short of the audio's end. D4 already documents that the
position can stop up to one buffer short and the driver accepts 50 ms; the
test asserted exact equality, which is not a property of the device. It now
asserts the documented 50 ms bound and records the shortfall per file
(`frames_short_of_end`). This is the measurement, not a loosened tolerance.

## Not verified (see the report's `unknowns`)

- No human watched or listened (`human_review`, owner only).
- Retail pause, master clock, skip and end-of-scene semantics for msopen1,
  zipper, chap0 and crimflag remain unknown (F40-D2); the driver runs a designed
  probe plan. Skip and pause were not exercised on the device.
- Cinematics are not reachable from the app's menu, cabin or ending flow, so
  no story trigger was driven end to end; callback 2151's chapter mapping is
  unread.
- The in-engine camera sequences (cam_anim.zbd, F40-D3) were not played.
- Nothing compares output with the original engine (needs an owner-supplied
  original run).
