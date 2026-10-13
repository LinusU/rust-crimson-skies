# F40-D1: no approved MPEG-1/MP2 decoder path exists — owner decision required

Date: 2026-10-13. Rally task #660 (`### F40-D1` of
`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`), blocked
before implementation. Capabilities on this host: `retail` (read-only
`$CS_GAME_DIR`), `gpu`, `audio`. Host tools: ffprobe/ffmpeg 8.1.1
(`/opt/homebrew/bin`, not a workspace dependency); `CS_FFMPEG` unset.

The task states its own gate: *"Needs an approved decoder dependency (or
documented private transcoding cache); if none is approved, block_task
asking the owner."* This note records that no approval exists, re-measures
the ten retail files so the owner can decide against fresh facts, and lists
the concrete decision options. Nothing here is an approval, an acceptance or
evidence.

## Why the gate is closed

Checked, and none exists:

- **Task history.** The F40-D split note (#167, 2026-10-05) measured the
  files and concluded: *"The repo has no MPEG-1/MP2 FrameSource, no approved
  decoder dependency … The owner must approve a decoder dependency (or
  private transcoding cache) before D1."* No approval note follows: #660's
  history holds only its creation and this session's start, and the Rally
  activity log from 2026-10-05 to 2026-10-13 contains no decoder/MPEG/codec
  approval by the owner.
- **Workspace dependencies.** Root `Cargo.toml` and `crates/cs_app/Cargo.toml`
  resolve Bevy 0.19, Avian3d 0.7, the eight `cs_*` crates, `rodio`
  (`playback` only — configured deliberately *without* codec features because
  the crate decodes original members itself, F06-C) and `image` (PNG/JPEG/
  TGA/BMP only — named because Bevy already compiles it into the graph,
  F45-D). Nothing in the dependency graph decodes MPEG-1 video or MP2 audio.
- **Prior findings.** `2026-10-05-f40-b-playback-and-camera-timelines.md`
  records the same open item: *"The original video container/codec and an
  approved decoder dependency or private transcoding cache: no real
  `FrameSource` exists yet."*

## Measured state of the ten retail files (2026-10-13, read-only)

`ffprobe` 8.1.1 over each file of
`$CS_GAME_DIR/GOSDATA/ASSETS/GRAPHICS/MPG/`, video packets counted with
`-count_packets`; the sequence header was then read directly from the bytes
(first `00 00 01 B3` start code in a 2 MB prefix read):

| file | video codec, size | seq-hdr frame_rate_code | video packets | duration s | audio codec, rate, ch | bytes |
| --- | --- | --- | --- | --- | --- | --- |
| chap0.mpg | mpeg1video 320x240 | 5 (30.000) | 4350 | 144.953467 | mp2 44100 2 | 18 696 196 |
| chap1.mpg | mpeg1video 320x240 | 5 (30.000) | 3928 | 130.821222 | mp2 44100 2 | 16 287 748 |
| chap2.mpg | mpeg1video 320x240 | 5 (30.000) | 3700 | 123.271833 | mp2 44100 2 | 15 360 004 |
| chap3.mpg | mpeg1video 320x240 | 5 (30.000) | 2786 | 92.734689 | mp2 44100 2 | 11 563 012 |
| chap4.mpg | mpeg1video 320x240 | 5 (30.000) | 4126 | 137.483333 | mp2 44100 2 | 17 147 908 |
| chap5.mpg | mpeg1video 320x240 | 5 (30.000) | 2876 | 95.816667 | mp2 44100 2 | 11 966 468 |
| crimflag.mpg | mpeg1video 320x240 | 5 (30.000) | 242 | 8.016667 | mp2 44100 **1** | 970 756 |
| final.mpg | mpeg1video 320x240 | 5 (30.000) | 3332 | 111.020400 | mp2 44100 2 | 13 568 004 |
| msopen1.mpg | mpeg1video 320x240 | **4 (29.97)** | 405 | 13.531422 | mp2 44100 2 | 2 664 262 |
| zipper.mpg | mpeg1video 320x240 | 5 (30.000) | 610 | 20.283333 | mp2 44100 2 | 2 535 428 |

Additional measured facts, recorded without interpretation:

- ffprobe reports `r_frame_rate 60/1` for all ten streams while
  `avg_frame_rate` is `30/1` (msopen1: `60000/1001` / `30000/1001`); the
  packet count over the reported duration agrees with the sequence-header
  rate to within about one percent in every file.
- The first sequence header sits at file offset `0x30` (`msopen1`: `0x31`) —
  directly after the pack/system headers, i.e. the files begin with an
  MPEG-PS pack, as the split note recorded.
- Every sequence header states `aspect_ratio_info = 1` (1:1). What the
  original player did with that byte (full-screen 4:3 presentation is F40's
  letterboxing territory) is unmeasured.

Reproduction (read-only):

```sh
cd "$CS_GAME_DIR/GOSDATA/ASSETS/GRAPHICS/MPG"
ffprobe -v error -select_streams v:0 -count_packets \
  -show_entries stream=codec_name,width,height,r_frame_rate,avg_frame_rate,nb_read_packets \
  -of csv=p=0 FILE
ffprobe -v error -select_streams a:0 -show_entries stream=codec_name,sample_rate,channels -of csv=p=0 FILE
ffprobe -v error -show_entries format=duration -of csv=p=0 FILE
# sequence header: first b'\x00\x00\x01\xb3'; next 12+12+4+4 bits are
# horizontal_size, vertical_size, aspect_ratio_info, frame_rate_code.
```

## The decision the owner needs to make

The spec (`F40` sheet) allows decoding *"through an approved dependency or
documented private transcoding cache"*. The four candidate routes, with what
each costs and unblocks:

- **A. Documented private transcoding cache.** ffmpeg (host tool, the
  `CS_FFMPEG` pattern of T444) transcodes each retail `.mpg` once into a
  documented, hashed intermediate under `private/` (Git-ignored; original
  data never committed); the production `FrameSource`/audio source read that
  intermediate. No runtime dependency and no licensing surface, but the
  cache exists only on machines that ran the transcode, so the decode path
  is untestable anywhere the cache is absent — AC04 then requires a
  `MediaFailure` naming the missing cache rather than a completion.
- **B. External ffmpeg/ffprobe at runtime.** The production path invokes an
  env-pointed binary (again the T444 precedent, which used ffmpeg as an
  independent cross-check, not as the production decoder). Decode
  correctness and output depend on the host's ffmpeg version; a game that
  requires an installed ffmpeg is a distribution decision for the owner.
- **C. Rust bindings to libav\*** (e.g. `ffmpeg-next`/`ffmpeg-sys-next`,
  system or vendored libavcodec/libavformat). A single-process decode path,
  at the cost of a compiled native dependency on CI and every dev machine,
  LGPL/GPL licensing and `THIRD-PARTY-NOTICES` implications (F61-A).
- **D. First-party native decoders.** MPEG-PS demux plus MPEG-1 video and
  MP2 audio decoding written in-repo, following the T444 pattern (first-party
  decode, optionally cross-checked against ffmpeg through `CS_FFMPEG` in the
  acceptance harness, evidence recording both). No new dependency —
  consistent with the deliberate no-codec `rodio` configuration and the
  F06-C self-decode culture — but it is a large body of correctness-critical
  codec code (IDCT, motion compensation, polyphase synthesis) whose fidelity
  risk the owner should weigh explicitly against the near-term milestone of
  one real original mission.

Whichever route is approved, the D1 acceptance is unchanged: `accept_f40_d_*`
tests calling production code, retail tests `#[ignore = "requires
CS_GAME_DIR"]`, frame count/duration/size compared against ffprobe for all
ten files (the table above is the fresh baseline), AC04's useful-error-no-
false-completion behaviour, and an evidence report per
`docs/contracts/CLI-EVIDENCE.md`.

## What this note is not

- It is not an approval, and it asserts no decode capability: F40-D1 is
  blocked, no production decode code exists, and no `accept_f40_d_*` test
  exists yet.
- No evidence report exists for F40-D1 (an acceptance harness cannot exist
  before an implementation); nothing here may be read as acceptance.
- Scene/trigger binding stays F14-D's open item
  (`2026-10-13-f14-d-video-chapter-binding.md`); no `chap<N>.mpg` row is
  minted or changed here.
- The byte facts above are container/sequence-header measurements only; no
  picture data, no audio samples and no original run were produced.

No file inside `$CS_GAME_DIR` was written, and no original bytes, frames or
audio are committed.
