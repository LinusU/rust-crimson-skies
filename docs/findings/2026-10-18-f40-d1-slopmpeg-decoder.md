# F40-D1: the retail cinematics decode through slopmpeg

Date: 2026-10-18. Rally task #660 (`### F40-D1`). Capabilities used: `retail`
(read-only `$CS_GAME_DIR`). It resolves the gate recorded in
`2026-10-13-f40-d1-mpeg-decoder-approval-required.md`.

## Decision

The owner approved route D with an existing crate: `slopmpeg`
(<https://github.com/LinusU/slopmpeg>, the owner's own Rust port of PL_MPEG:
MPEG-PS demux, MPEG-1 video, MP2 audio; no dependencies, no unsafe code) as a
regular git dependency of `cs_app`. `Cargo.lock` pins the revision. The
approval is the owner note on #660 of 2026-10-18.

## What exists now

`cs_app::cinematics::mpeg`:

- `MpegVideoSource` is a `FrameSource`: frames in presentation order with
  their time in microseconds, the pixels of the last delivered frame as RGBA.
- `MpegAudioSource` delivers interleaved stereo `f32` samples (mono `crimflag`
  arrives as two identical channels) and the stream's sample rate.
- A missing file is `MediaFailure::MissingMedia`, a file that is not an MPEG-1
  program stream is `MissingDecoder`, and anything else the decoder rejects or
  a failed read is `DecodeFailed`: never a completion (AC04).

Nothing consumes the sources yet: the renderer and audio device wiring is
F40-D4 (#663).

## Measurements on the retail installation

All ten files decode to 320x240 video at 44.1 kHz audio. Video frame counts
equal the number of coded pictures counted straight from each file's
bitstream (picture start codes in the video packets, I+P+B), and the audio
length and frame rate agree with the 2026-10-13 ffprobe baseline.

| file | coded pictures (= frames decoded) | ffprobe packets (2026-10-13) |
| --- | --- | --- |
| chap0 | 4349 | 4350 |
| chap1 | 3927 | 3928 |
| chap2 | 3699 | 3700 |
| chap3 | 2785 | 2786 |
| chap4 | 4125 | 4126 |
| chap5 | 2875 | 2876 |
| crimflag | 241 | 242 |
| final | 3331 | 3332 |
| msopen1 | 405 | 405 |
| zipper | 609 | 610 |

ffprobe's packet count is one higher in nine files; the extra packet is not a
coded picture (no matching picture start code exists in the stream). Whether it
is a trailing sequence-end packet was not examined.

## A decoder defect found and fixed upstream

slopmpeg (like PL_MPEG) dropped the last frame of nine of the ten files: when
a stream ends on a B-frame, the reference frame waiting to be displayed after it
was never returned. Fixed in slopmpeg `393f420` (the pending reference frame is
returned at the end of the stream whatever the last decoded picture was); its
Crimson Skies regression counts and hashes were updated, and `cs_app` pins that
revision.

## What stays unknown

- Pixel and sample fidelity against an independent decoder: slopmpeg is checked
  against PL_MPEG's hashes (its own suite) and against the bitstream's picture
  count only; there is no ffmpeg on this host. Resolved by a frame-hash
  comparison against ffmpeg or the owner's review.
- The decoder's duration (the video stream's last timestamp) differs from
  ffprobe's container duration by up to 0.3 s (audio tail); which one the
  original used for the end of playback is unmeasured (F40-D4).
- The original engine's own handling of the last frames was not compared.
