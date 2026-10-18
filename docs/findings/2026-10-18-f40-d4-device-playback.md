# F40-D4: original cinematics on the real audio clock and renderer

Date: 2026-10-18. Rally task #663 (`### F40-D`, prefix `accept_f40_d4_`).
Capabilities used: `retail`, `gpu`, `audio` (an Apple M3 Pro, Metal; the default
output device). Evidence: `docs/findings/evidence/F40-D4.json`.

## What exists now

`cs_app::cinematics::device::play_on_device` connects the F40-D1 sources, the
F40-C `CinematicSession` and the real output device:

- the file's MP2 audio is decoded to one voice on the `AudibleDevice`, opened
  behind the `audio` capability gate (without it: a named `DeviceError`, nothing
  played);
- **the master clock is how far the output stream has pulled that voice**, in
  audio frames; the session is stepped with the frames the device played, and
  the video follows (`VideoPlayback::frame_due`), so a stalled output stalls the
  picture and never completes the scene (`DevicePlaybackError::DeviceStalled`);
- every frame the session shows is composed by `compose_letterboxed` into the
  output surface at the source's own aspect (the `fit_letterboxed` viewport,
  nearest scaling, opaque black bars): 320x240 into 1280x720 is 960x720 at
  x=160, never stretched;
- a `FramePresenter` receives each composed frame; a missing file or a file that
  is not MPEG-1 is a `MediaFailure`, never a completion.

`cs_app::cinematics::frame_capture::capture_surface` draws a composed surface
one-to-one on the real GPU (offscreen target, nearest sampling), reads it back,
compares it pixel for pixel with the uploaded surface and writes a PNG.

## Measurements (retail, this machine, debug build)

| file | audio frames played / total | frames presented | max drift | wall |
| --- | --- | --- | --- | --- |
| crimflag | 353664 / 353664 | 202 of 241 | 31.7 ms | 8.30 s |
| msopen1 | 596736 / 596736 | 334 of 405 | 49.9 ms | 13.83 s |
| zipper | 893952 / 893952 | 522 of 609 | 31.7 ms | 20.57 s |

Nine sampled surfaces (three per file) read back identical to the uploaded
surface, with the bars exactly black and the picture lit. Hashes are in the
evidence report.

- The output position stops up to one buffer short of the audio's end (the last
  partial buffer is never reported). A position within 50 ms of the end that has
  not advanced for 250 ms is taken as the end of the audio. This is a measured
  property of the `rodio` position, not of the original.
- Frames are dropped to stay on the audio clock when a poll takes longer than a
  frame (about 15-17 % in a debug build; the dropped frames are counted by the
  session, not hidden). Drift stayed under the designed 100 ms tolerance.

## Not retail semantics

The retail pause policy, master clock, skippability and end-of-scene semantics
are unknown (F40-D2), so no retail plan can be lowered. The driver runs a
**designed playback-probe plan** (length = decoded audio length, not skippable,
no actions). Device pause is not implemented.

## Unmet (see the report's `unknowns`)

- `human_play` / `human_review`: nobody watched or listened; owner only.
- chap0-chap5 and final were not played on the device here.
- Frames are rendered offscreen, not in the game window, and the cinematics are
  not yet reachable from the app's flow.
- Pause, skip and failure recovery on the device.
- No comparison with the original engine's output (needs an owner-supplied
  original run).
