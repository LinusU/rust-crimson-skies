# Task #344: RIFF/WAVE headers of the sound archive members

Date: 2026-09-28. Task #344 "Fill SoundDescriptor from the RIFF/WAVE headers
of retail sound archive members", a follow-up to tasks #340 and #343
(`docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`,
`docs/findings/2026-09-28-t343-zbd-version-one-member-index.md`).
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t344_`. Evidence:
`docs/findings/evidence/T344.json`.

## Sources

- **RIFF/WAVE layout**: IBM Corporation and Microsoft Corporation,
  *Multimedia Programming Interface and Data Specifications 1.0*, August
  1991, sections "RIFF File Format" (chunk = id, u32 size, payload, pad byte
  when the size is odd; the `RIFF` size counts the form type and all chunks)
  and "WAVE Form Type" (`fmt ` before `data`; the common `fmt ` fields
  `wFormatTag`, `nChannels`, `nSamplesPerSec`, `nAvgBytesPerSec`,
  `nBlockAlign`, then the format-specific `wBitsPerSample`; the `cue ` chunk
  as `dwCuePoints` followed by 24-byte cue points).
- **Format tag names**: RFC 2361, "WAVE and AVI Codec Registries" (1998),
  appendix A: `0x0001` `WAVE_FORMAT_PCM`, `0x0002` `WAVE_FORMAT_ADPCM`
  (Microsoft), `0x0011` `WAVE_FORMAT_DVI_ADPCM` (Intel, IMA ADPCM).
- **Retail installation**: install fingerprint
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`, the
  same installation tasks #340 and #343 used. The per-archive table (fmt
  shapes with member counts, cue and `smpl` counts, failures, independent-read
  mismatches) is the evidence artifact `zbd-sound-wave-headers.json`. It is
  private and the report references it by digest.

## Code

`crates/cs_formats/src/zbd/wave.rs`: `read_wave_header(member)` reads one
member's header. It needs no allocation and checks every read against the
member slice. Offsets are relative to the member's first byte.

- It checks `RIFF`, `WAVE` and that the `RIFF` size + 8 is the member length.
  Then it walks the chunks. Every payload must fit, and an odd payload is
  followed by a pad byte.
- `fmt ` must be at least 16 bytes. Any format-specific tail (the ADPCM
  coefficient data) is kept inside `fmt_span` and not interpreted.
- `data` must come after `fmt `. `fmt `, `data`, `cue ` and `smpl` may each
  occur once. Other chunks are counted and skipped.
- `cue ` must hold its declared count of 24-byte points. Only the count is
  reported.
- Every failure is a `WaveError` with a stable `code()`, a static `reason()`
  and a `Display` form that names ids, sizes and offsets, never sample bytes.

`crates/cs_formats/src/zbd/sound_archive.rs`: `SoundEntry::wave()` returns
the header or the error. `SoundEntry::descriptor()` is built from it:

| Field | Value |
| --- | --- |
| `format` | `pcm`, `ms_adpcm` or `ima_adpcm`; any other tag is unknown (`UNNAMED_FORMAT_REASON`) |
| `format_tag` | `wFormatTag` |
| `channels` | `nChannels` (now `u16`, the width the header declares) |
| `rate_hz` | `nSamplesPerSec` |
| `bits_per_sample` | `wBitsPerSample` |
| `block_align` | `nBlockAlign` |
| `cue_points` | `dwCuePoints`, or 0 without a `cue ` chunk |
| `loop_points` | always unknown, see below |

A member whose header does not read keeps every field unknown, with the
error's reason. `SoundArchive::wave_failures()` lists those members next to
their siblings. `SoundArchive::status()` still reports only the bounds of
the members, as its documentation says, so a strict audit (F06-D) must count
both. `unsupported_records()` still lists every readable entry, because no
sample is decoded. The reason is the `WaveError`, or
`SAMPLES_NOT_DECODED_REASON` for a member whose header reads.

## Retail results

Every member of both archives reads: 2520 in `ZBD/soundsl.zbd` and 2521 in
`ZBD/soundsh.zbd`, with no `WaveError`. In every member `fmt ` is the first
chunk. The test compares the production fields against an independent read
of offsets 20..36 and finds no mismatch. The chunk lists are `fmt `, `data`
or `fmt `, `cue `, `data`. No other chunk and no `smpl` occurs.

`fmt ` shapes (tag, channels, rate, bits per sample, block align → members):

| Archive | Shape | Members |
| --- | --- | --- |
| soundsl | IMA ADPCM, 1, 11025, 4, 256 | 555 |
| soundsl | MS ADPCM, 1, 11025, 4, 256 | 1954 |
| soundsl | PCM, 1, 11025, 8, 1 | 11 |
| soundsh | MS ADPCM, 1, 11025, 4, 256 | 1954 |
| soundsh | MS ADPCM, 1, 22050, 4, 512 | 470 |
| soundsh | MS ADPCM, 2, 22050, 4, 1024 | 58 |
| soundsh | MS ADPCM, 1, 44100, 4, 1024 | 24 |
| soundsh | MS ADPCM, 1, 22000, 4, 512 | 3 |
| soundsh | MS ADPCM, 2, 9710, 4, 512 | 1 |
| soundsh | PCM, 1, 22050, 16, 2 | 8 |
| soundsh | PCM, 1, 11025, 8, 1 | 1 |
| soundsh | PCM, 2, 22050, 16, 4 | 1 |
| soundsh | PCM, 2, 22050, 8, 2 | 1 |

Unusual encodings, recorded as found: IMA ADPCM appears only in `soundsl`.
The MS ADPCM `fmt ` payloads are 50 bytes (`cbSize` 32: samples per block and
the seven coefficient pairs). The IMA ADPCM payloads are 20 bytes
(`cbSize` 2: samples per block). The odd rates 22000 Hz and 9710 Hz are read
as declared. Nothing here decides whether they are intended.

## Unknowns

- **Loop points.** No retail member carries a `smpl` chunk, so no WAVE
  header declares a loop region, and `loop_points` stays unknown for every
  member (`NO_LOOP_CHUNK_REASON`). Whether the game loops a sound (engine
  hum, gunfire) is decided outside the WAVE header and is not known. A
  member with a `smpl` chunk would keep it located (`smpl_span`) but unread
  (`SMPL_NOT_READ_REASON`). No retail member provides a layout to check it
  against.
- **Cue points.** 35 members per archive carry a `cue ` chunk with 222
  points in total. They are 24 `*_briefing.wav` narrations and 11 effects.
  Nine of the effects carry one point (`warning_beeper.wav`,
  `windowhit1..3.wav`, `thunder.wav`, `freighter_aground.wav`, `train2.wav`,
  `pilot_eject1.wav`, `gull_hit.wav`) and two carry two (`mechmove.wav`,
  `hookcontact.wav`). A cue point is a single position, not a region, so none
  of them is read as a loop. Only the count is reported. In `soundsl` the `dwPosition` and
  `dwSampleOffset` of a point differ by a factor of four, and in `soundsh`
  they are equal. What the game does with these points (for example syncing
  briefing slides) is unknown.
- **Sample decoding.** It is not done here. Decoding PCM, MS ADPCM and IMA
  ADPCM and comparing the decoded sample count with the declared format is
  F06-C's AC03.
